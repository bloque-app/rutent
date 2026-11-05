---
name: Compliance proofs, provider assertions and regulatory obligations
status: informative
date: 2026-11-02
---

# Compliance proofs, provider assertions and regulatory obligations

## Basis

**This is not legal advice.** I read regulations as an engineer's research aid. Where a section says "reading", it is my reading of a text, and a lawyer in that country could disagree. Nothing here settles whether anyone is regulated.

**Read on 2026-11-02, inside this project:** `CLAUDE.md`, `PRODUCT.md` (`PS-13`, `PS-15`, `PS-21`, `PS-22`, `PS-30`, `PS-31`), `SPEC.md` (`REQ-CPL-*`, `REQ-AUZ-*`, threat area "False compliance proofs"), `profile.oss-ledger.md`, and `research/2026-11-02-durable-state.md`. Its section 3 covers audit evidence and is not repeated here; its section 2 is cross-referenced for check-then-act and for compensation on rails.

**Read on 2026-11-02, outside this project:** only public external sources, listed under References with the version and date each shows. Nothing from the directories the brief excludes was opened, and no production system or admin tool was queried.

**How sources were read.** There are three grades, and they differ in reliability:
- **Digest.** Web pages and most regulator texts went through a fetch tool that returns a model-written digest of the page, so a digest can misread its page. Long pages were read only in the parts named. This applies to every W3C, IETF and OpenID document, and to the Banco Central do Brasil (BCB) norms.
- **Direct.** These were extracted as text or rendered as pages and read by me: the FATF Recommendations in the GAFILAT Spanish edition (© 2018 footer, file labelled December 2019, PDF built in 2020); the FATF 2021 updated guidance in Spanish, as hosted by Mexico's tax authority; the BCB press note of 2025-11-10; pages 1 to 8 of the UK FCA final notice cited below; Banco de México Circular 9/2026 as printed in the Diario Oficial; two Mexican law-firm notes; and the Mexican tax authority's 2026 FAQ.
- **Search excerpt.** A claim marked *(search excerpt)* comes only from a search result's excerpt and its page was not opened. Treat those as leads.

**Access limits that shape this report.** The FATF site returned a bot challenge for every request, so I could not read the current English Recommendations, the revised Recommendation 16 text, its explanatory note, or the July 2026 targeted update. I did not try to get past the challenge. FATF texts below come from the older Spanish copies, or from secondary sources where marked. The Spanish is paraphrased in English by me. Banco de la República pages returned a similar bot redirect, so Bre-B is the weakest of the three rails here.

**Orchestrator's review, 2026-11-02.** Checked at the source: the Galicia note's facilitation wording and its 210 UMA threshold, read directly from the PDF, match section 3; W3C VC 2.0's status and its "does not imply the truth of claims" line. The Mexican wording is still one law firm's account, unconfirmed in the official DOF text. Nothing here is legal advice, and no legal claim here should reach `SPEC.md` as fact without counsel.

**Tags.**
- **Source:** the cited document says this.
- **Measured:** the cited document reports this as a measurement, under the conditions given.
- **Inference:** my reasoning from sources. It is not stated by any of them.

Direct quotation is kept to one phrase; everything else is paraphrase. This report decides nothing. It names no winner and writes no requirement. Vendors are described generically.

## Findings that matter most

1. **No mechanism proves a user "may transact". Each proves that a named party said something at a time.** The W3C model says verification "does not imply the truth of claims". So `PS-22` can be met as "this provider asserted this, and cannot deny it later", and not as "this is true". (Source and Inference, sections 1 and 2.)
2. **Regulatory responsibility did not move in any regime I read.** The party that relies on or delegates to another stays responsible under FATF, the EU, the UK, the Colombian supervisor's 2025 concept and two Brazilian norms. FATF limits "third parties" to supervised institutions. A reserve vouching to an unsupervised rutent is therefore a contractual statement, not regulatory reliance. (Source and Inference, section 2.)
3. **"We only instruct licensed providers" is not a safe position to assume.** FATF applies a functional test that covers intermediaries and anyone with control or sufficient influence. A Mexican law-firm note on the August 2026 rules reads them as capturing platforms that connect, reconcile or match operations, even without holding the assets and even if limited to local-money flows. That reading needs a lawyer and the primary text. (Source, secondary, and Inference, section 3.)
4. **Obligations can reach rutent by contract even if it is not regulated.** Brazilian text makes a licensed provider diligence, disclose and open to the supervisor any technology specifically related to its virtual-asset services. This is the most concrete trigger found for a `reg-fintech`-type evidence map. (Source and Inference, section 3.)
5. **A repeated yes-or-no check leaks.** Payment-channel balances have been recovered this way in under a minute per channel, and Brazil already requires controls against scanning its key directory. `PS-31` as written does not by itself protect a balance. (Measured, Source and Inference, section 5.)

## 1. How a party proves that a user may transact

### What `PS-22` asks, read closely

Inference. "Properly able to enact a set of transactions" bundles at least five different questions: who the user is, whether they were checked, whether they are currently allowed for this kind of transaction in this jurisdiction, whether they appear on a sanctions list, and whether this request really comes from them. Standards address pieces of this. None answers it whole. An eligibility answer is a policy result that a provider computes from those pieces.

### Comparison

| Mechanism | What is proven | By whom | To whom | What the verifier learns | Revocation and freshness | Maturity and deployment |
|---|---|---|---|---|---|---|
| W3C Verifiable Credentials 2.0 | An issuer made these claims about a subject; integrity of the claims | Issuer signs; holder presents | Any verifier | The claims presented, issuer identity, the proof; identifiers can correlate | `validFrom`/`validUntil`, optional `status` entry | W3C Recommendation, 2025-05-15 |
| SD-JWT, RFC 9901 | Disclosed claims were signed by the issuer; optional key binding to the presenter | Issuer signs; holder selects | Any verifier | Disclosed claims plus anything left in plaintext; presentations link unless each is used once | No status mechanism in the RFC part read | Standards Track, November 2025. SD-JWT VC is draft-23, in IESG evaluation, telechat 2026-11-05 |
| BBS cryptosuite | Same, with proofs unlinkable to each other and to the original signature | Issuer signs; holder derives | Any verifier | Revealed claims, the issuer key, count of messages, revealed indexes | Validity fields; status not established in the parts read | W3C Candidate Recommendation Draft, 2026-09-10. Underlying scheme is draft-irtf-cfrg-bbs-signatures-14, not an RFC, no IETF standing |
| OpenID for Verifiable Presentations 1.0 | Nothing itself; it carries a presentation and binds it to verifier and nonce | Wallet | Verifier | What was requested and consented to | Verifiers run the checks their trust policy requires, revocation being an example; nonce covers replay | Final, 2025-07-09. Supports W3C VC, ISO mdoc, SD-JWT VC |
| Bitstring Status List 1.0 | Whether one credential's status bit is set | Status publisher | Any verifier | The bit; publisher may learn who checks unless cached or proxied | Optional `ttl`, no default; list is itself a credential | W3C Recommendation, 2025-05-15 |
| Token Status List | Same idea for JOSE and COSE tokens, including SD-JWT and mdoc | Status provider | Relying party | The status; herd privacy depends on list size | `ttl` and `exp` claims | draft-21, 2026-06-21, submitted to the IESG; the datatracker shows it in an RFC Editor state, not yet an RFC |
| Zero-knowledge proof over an existing signed credential | A statement about hidden credential fields; for example, over 18 | Holder's device, from an unmodified issuer credential | Any verifier | The statement only, if the proof is sound and the holder reveals nothing else | Inherits the credential's validity; status not established | Published paper and open-source library; see below. Age checks are the reported use, not KYC |
| Published attestation on a public chain | A named company attests that a wallet belongs to a verified account | That company | Anyone who reads the chain | Wallet is linked to a verified customer, permanently public | Issuer can update; not read in detail | Deployed by one large exchange on its own chain *(search excerpt)* |
| Reliance under FATF Recommendation 17 | A supervised party performed customer checks | Supervised third party | Another obliged institution | Identity information, immediately | Copies on request without delay | Long-standing regulatory arrangement |
| Outsourcing of customer checks | Checks done under the institution's own procedures | Contractor | The institution itself | Everything | The institution's own | Permitted with limits, for example in Colombia |
| Verifiable LEI | A legal entity's identity and the role of a person acting for it | Qualified issuers under a root operated by GLEIF | Any verifier | Entity and role | Issuance and revocation policies per credential; mechanics not read | Governance Framework v4.0, primary document 2026-03-25 |
| Agent mandate credentials | A user authorised this agent for this purchase within these conditions | User, then agent | Merchant or provider | The mandate | Not read | Launched September 2025, later moved to an industry alliance *(search excerpts, vendor blogs; formats conflict)* |

### Notes on each

**W3C VC 2.0.** Source: the roles are issuer, holder and verifier, and a holder is often but not always the subject. Verification does not establish that claims are true; verifiers apply their own rules. Section 8.1 ranks identifiers from highly correlatable to pseudonymous, and notes that a bare proof of age may not suffice for some regulated uses. Section 8.3 advises assuming that a credential leaks personal information when shared ([VC 2.0](https://www.w3.org/TR/vc-data-model-2.0/)). I read sections 1 to 4 headings, and 8.1 to part of 8.4, not the remainder.

**SD-JWT.** Source: the issuer signs digests of selectively disclosable claims, so a holder can omit some claims without breaking the signature. Key binding is enforced by verifier policy, not by the holder choosing to send it. Section 10.1 separates four kinds of unlinkability. Batch issuance with new keys and salts helps presentation and verifier-to-verifier unlinkability. Issuer-to-verifier unlinkability is achievable only with an honest verifier, since callbacks and timing can still reveal use, and it cannot be achieved with a careless, colluding, compromised or coerced verifier, because the verifier holds the issuer-signed credential. Decoy digests hide how many claims exist, at a size cost ([RFC 9901](https://www.rfc-editor.org/rfc/rfc9901.html)).

**BBS.** Source: derived proofs are unlinkable even for identical disclosed data. The suite uses BLS12-381. Its security rests on discrete-log assumptions that are not post-quantum. The optional holder-binding and pseudonym features are flagged at risk of removal if the IETF work does not reach RFC on time. The privacy section warns that message counts, revealed indexes, proof options such as `created`, disclosed claims and network addresses can all narrow the anonymity set ([W3C CRD](https://www.w3.org/TR/vc-di-bbs/), [CFRG draft-14](https://datatracker.ietf.org/doc/draft-irtf-cfrg-bbs-signatures/)). Inference: pairing support in mainstream key stores and certified hardware is a real adoption constraint. I did not measure it.

**OpenID4VP.** Source: the query language lets a verifier ask for specific claims and alternative combinations, and value matching is only a privacy aid that verifiers must not rely on for security. Verifiers must themselves decide whether an issuer is trusted. Without holder binding there is no replay protection and the verifier accepts the risk that the holder is not the subject (section 14.1.1). Presentations can be linked across verifiers through the issuer signature or bound key; wallets can use each credential instance once per verifier (section 15.5) ([OpenID4VP 1.0](https://openid.net/specs/openid-4-verifiable-presentations-1_0.html)).

**Status lists.** Source: credentials share one list so a holder hides among others. The Bitstring minimum is 131,072 entries unless an ecosystem sets another bound, and small populations weaken the protection. A verifier that knows an index can watch it change. Verifiers should cache lists or use proxies to hide retrieval from the issuer. The specification sets no minimum or maximum validity period, and its privacy text is non-normative ([Bitstring](https://www.w3.org/TR/vc-bitstring-status-list/), [Token Status List](https://datatracker.ietf.org/doc/draft-ietf-oauth-status-list/)). Inference: an operator with a few thousand end users per provider is exactly the small population these documents warn about.

**Zero-knowledge attestations.**
- Measured, by the authors: ECDSA proofs in 60 ms and an mdoc presentation proof in 1.2 s on mobile, varying with credential size *(search excerpt)* ([Frigo and shelat, eprint 2024/2010](https://eprint.iacr.org/2024/2010)). Presented as needing no change to issuers.
- Source: a large technology company opened its zero-knowledge libraries on 2025-07-03, framed around EU age assurance and a German savings-bank partnership *(search excerpt)* ([post](https://blog.google/technology/safety-security/opening-up-zero-knowledge-proof-technology-to-promote-privacy-in-age-assurance/)).
- I found no regulator accepting a zero-knowledge proof as customer due diligence, and no named regulated institution using one for KYC in production. Results were vendor claims, test-network demos and academic papers (for example [zkKYC, eprint 2022/321](https://eprint.iacr.org/2022/321)) *(search excerpts)*. That is an absence in one search, not proof of none.
- Inference: these prove a predicate over a credential. They do not make the underlying check any better, and FATF reliance expects information to reach the relying party (section 2).

**Reusable KYC and wallets.**
- Source: under the EU AML Regulation reliance is limited to obliged entities and supervised equivalents, with ultimate responsibility staying with the relying entity ([AMLR Art. 48](https://www.springlex.eu/en/packages/aml/amlr-regulation/article-48/)).
- Source, weak: EU member states must offer a digital identity wallet by end-2026, and AML Regulation obligations apply from 2027-07-10. Commentary says a wallet satisfies identity verification only, not the rest of due diligence such as screening, and sources disagree on whether wallet acceptance binds onboarding *(search excerpts, vendor and law-firm blogs)* ([one commentary](https://www.entrust.com/blog/2026/02/eu-compliance-regulations)). The EU is outside rutent's named rails and appears only as the most advanced regulated example.
- I found nothing primary on national KYC utilities in the time available, and nothing on Colombia, Brazil or Mexico accepting reusable credentials from a private issuer. The results were dominated by vendor marketing.

### Reading across

Inference.
- The standards are mature for "a named issuer signed these fields" and young for unlinkability. Only SD-JWT, VC 2.0, Bitstring and OpenID4VP are final.
- The wallet model assumes a holder with a device and key. `PS-22` as worded has the reserve proving on behalf of a user, with no holder step at all. In that shape, most of the selective-disclosure and unlinkability machinery has no one to run it, and the proving party is the provider itself. Whether end users present anything is an open owner question.
- `PS-2` puts agents among the users. Binding an agent's authority to a human or legal principal is an emerging field with no settled standard in what I read, and I found no regulator guidance on it.
- Entity-level proof for providers themselves (`PS-22`'s "actors") is a different and easier problem than end-user proof, since entities are not owed the same anonymity.

## 2. What a provider's assertion is really worth

### Reliance rules

**FATF Recommendation 17.** Source, GAFILAT Spanish edition, read directly and paraphrased ([compendium](https://pplaft.cnbs.gob.hn/wp-content/uploads/2020/10/40-Recomendaciones-Actualizacion-Dec-2019.pdf)):
- Countries may let institutions rely on third parties for customer identification, beneficial-owner identification and understanding the purpose of the relationship, or to introduce business. Ultimate responsibility stays with the institution that relies.
- The relying institution must immediately obtain the necessary information.
- It must take steps to ensure the third party will supply copies of identification data on request and without delay.
- It must satisfy itself that the third party is regulated, supervised or monitored and has measures in place for due diligence and record keeping.
- Countries consider the country risk of where the third party sits.
- The Interpretive Note says the Recommendation does not cover outsourcing or agency. A true third party usually has its own existing relationship with the customer and applies its own procedures. "Third parties" means supervised or monitored financial institutions or designated non-financial businesses.

I read this edition, not the current one, and could not confirm that the text is unchanged.

**FATF 2021 guidance.** Source, Spanish, direct: reliance does not exempt the obligated party from its duties or from responsibility for non-compliance (¶205). Countries may let VASPs act as third parties, and institutions should choose them on risk, including the third party's business, customers and partners (¶206) ([guidance](https://www.sat.gob.mx/minisitio/ActividadesVulnerables/documentos/Normateca/Internacional/EnfoqueBasadoenRiesgo_activosvirtuales.pdf)).

**The same shape in national texts.**

| Regime | Rule | Source |
|---|---|---|
| EU | Ultimate responsibility remains with the relying obliged entity | [AMLR Art. 48](https://www.springlex.eu/en/packages/aml/amlr-regulation/article-48/) |
| UK | The relying person remains liable; must obtain information immediately and arrange copies on request; agents and outsourcers are treated separately, liability staying put | [MLR 2017 reg. 39](https://www.legislation.gov.uk/uksi/2017/692/regulation/39) |
| Colombia | A supervised entity may contract out customer-knowledge work under its own procedures. The decision to onboard cannot be delegated. Compliance-officer functions and identifying and reporting unusual and suspicious operations cannot be contracted out | [Superfinanciera concept 2025076970-002, 2025-05-29](https://www.superfinanciera.gov.co/publicaciones/10115675/boletin-juridico-no-116/) |
| Brazil, BaaS | The authorised provider is responsible for customer identification, qualification, fraud prevention and AML controls, and stays responsible when delegating accessory tasks. Accounts must be in the client's name at the provider | [Joint Resolution 16, 2025-11-28, Arts. 4, 9, 10](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20Conjunta&p2=16) |
| Brazil, virtual assets | The contracting provider stays responsible for what its contracted entities do (Arts. 35 and 36) | [Resolution fetched as BCB 520](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=520) |

### What follows for rutent

All Inference.

- **If rutent's operator is not an obliged entity,** a reserve's statement to it is not Recommendation 17 reliance, because the operator is not a relying institution. It is a contractual representation. Its value is what the contract and evidence give it: a remedy against the provider, and a record.
- **If rutent's operator is an obliged entity,** a bare yes does not look like it meets Recommendation 17's conditions. These expect identity information to reach the relying party immediately and copies on request. That collides with `PS-30` and with a design in which rutent holds no identity data. The operator would need its own relationship, or a different legal structure.
- **Either way, rutent cannot be the third party a licensed provider relies on** unless it is itself supervised.
- **Nothing here moves liability from the provider to anyone else.** That helps `PS-30`: the party with the duty is the party with the data. It also means rutent's assertion "this user was verified" is second-hand and should be labelled as such.

### Incentives and error

**Incentive.** Inference: a vouching party earns from volume and bears the cost of checks and refusals. Each false yes carries a small immediate gain to it and a delayed, uncertain cost. Rutent adds no regulator of its own, so the sanction is the provider's own supervisor, or removal from the catalogue.

**Even a supervised party can be wrong without knowing.** Source, read directly: a UK digital bank found in January 2023 that since 2017 its automated screening had checked customers against only a fraction of the UK sanctions list. It also came under a requirement on 2021-09-17 not to open accounts for high-risk customers, and the regulator found 54,359 accounts opened for 49,183 such customers in breach of it, within a relevant period of 1 December 2019 to 30 November 2023. The regulator fined it £28,959,426, after a 30 percent settlement discount ([FCA final notice, 2024-09-27, ¶¶1.1, 1.2, 2.3, 2.5, 4.14](https://fca.org.uk/publication/final-notices/starling-bank-limited-2024.pdf)). The FCA notice's own definition of the list and period is at its section 3. Inference: an honest "screened: yes" from that bank during those years would have been false, and no signature check would have shown it.

**Licensed partners answer for their fronts.** Source: US supervisors have issued a run of orders against banks over fintech partner oversight, including requirements that onboarding through partners and sub-partners meet AML rules *(search excerpts)* ([one summary](https://www.consumerfinancemonitor.com/2024/04/09/recent-fdic-consent-orders-show-increased-regulation-scrutiny-of-bank-relationships-with-fintech-partners/), [annual report of one bank](https://www.sec.gov/Archives/edgar/data/842717/000095017025036301/brbs-20241231.htm)). This is outside the three named countries. I read no order itself.

**When an intermediary hides its clients.** Source: Brazil now requires payment institutions to close an account whose holder provides financial or payment services outside the rules. Its example is using account funds to make, receive or settle payments for third parties in a way that can hide those parties' obligations and make them hard to identify. Documentation must be kept at least ten years. In force 2025-12-01 ([BCB 518, Art. 13](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=518)). The press link this to pooled accounts and a 2025 fuel-sector operation against organised crime *(search excerpts)* ([commentary](https://www.conjur.com.br/2025-set-18/brechas-regulatorias-fintechs-e-o-desafio-do-combate-a-lavagem-de-dinheiro//?print=1)). Inference: a rutent order presented to a rail under one operator-held account, with many end users behind it, resembles what this text targets.

### What can be detected, and by whom

Inference, built on the above and on section 3 of the durable-state report.

| Failure | Who can detect it | How | Who cannot |
|---|---|---|---|
| Forged or altered statement | Any verifier | Signature check | |
| Statement replayed for another request | Verifier, if bound to request and audience | Binding and nonce | Anyone, if unbound |
| Stale statement | Any verifier, if it carries a time and status | Comparison with a stated limit | Anyone, if time is absent |
| Provider withdrew approval, and rutent kept using it | Provider and a later auditor | Status plus an audit trail | rutent, if it never rechecks |
| Provider never checked the user | Provider's supervisor or auditor, by sampling underlying files | Only with access to files | rutent, by construction under `PS-30` |
| Provider checked, and the check was wrong | Supervisor, sometimes a sanctions match or a fraud report | After the fact | Most parties, in real time |
| Provider lost its licence | Anyone reading the public register, if one exists and is checked | Periodic check | Anyone who assumes it once and forever |
| One person vouched under different references by two providers | Only a party able to correlate them | Common identifier | rutent, unless `PS-30` is reopened |
| Fronting for others | Provider's monitoring, and partly rutent from volume and timing | Behavioural signals | Cryptography of any kind |
| Providers colluding | External audit, the rail's records | Comparison across independent records | Parties inside the collusion |

Reading across: cryptography can catch forgery, replay, staleness and silent edits. It catches none of the failures of truth. What it can do is make a false statement undeniable afterwards, which changes the incentive rather than the detection rate. That is the honest reading of "False compliance proofs" in `SPEC.md` section 8.

## 3. Regulatory obligations

Readings, not advice. Each jurisdiction is moving; several of the texts below changed during 2026.

### FATF generally

**Who is a virtual-asset service provider.** Source, FATF 2021 guidance in Spanish, read directly ([guidance](https://www.sat.gob.mx/minisitio/ActividadesVulnerables/documentos/Normateca/Internacional/EnfoqueBasadoenRiesgo_activosvirtuales.pdf)). All paraphrased:
- ¶55: a person who, as a business, for or on behalf of another, does one or more of: exchange between virtual assets and fiat; exchange between virtual assets; transfer; safekeeping or administration; participation in financial services related to an issuer's offer or sale.
- ¶56: the definition is read broadly and functionally. Self-description and technology do not decide it.
- ¶60: "as a business" excludes infrequent, non-commercial activity.
- ¶63: exchange is covered whatever role the provider plays toward users, including another intermediary that facilitates the transaction. A provider need not perform every element.
- ¶67: software is not itself a provider, but creators, owners and operators with control or sufficient influence may be, even if other parties play a role or the process is automated. Indicators include control over assets or protocol, a continuing business relationship with users, benefit from the service, and the ability to set or change parameters.
- ¶70: order-book services that match users' orders are listed as a business model. A platform that only lets buyers and sellers find each other and performs none of the listed services would not be a provider.
- ¶76: in the safekeeping discussion, firms that only provide ancillary infrastructure, such as cloud storage or signature-integrity checking, and developers who only build or sell software, usually fall outside. Countries still examine the facts.
- ¶82: someone who only creates or sells an application or platform may not be a provider. Using it to perform covered functions as a business changes that. Directing its development so as to provide services, especially while keeping control or sufficient influence over assets, software, protocol, platform or ongoing user relationships, probably qualifies.

Inference, applied to rutent. This cuts two ways, and which way depends on deployment:
- The open-source project, publishing code, looks closest to ¶76 and ¶82's first sentence.
- An operator running it for customers looks closer to ¶63 and ¶67. Several `PRODUCT.md` goals read as indicators of influence: choosing the provider catalogue (`PS-14`, `PS-18`), accepting signed modules (`PS-20`), posting quotes and routes (`PS-21`), and setting policy (`PS-13`).
- `PS-23`'s order books map directly onto ¶70's named business model.
- This also bears on `oss-ledger-2`, the split between project guarantees and operator obligations.

**Travel Rule.** Source, Interpretive Note to Recommendation 15, ¶7, GAFILAT edition, direct:
- Occasional-transaction due diligence applies at USD/EUR 1,000.
- The originating provider obtains and holds accurate originator data and required beneficiary data, sends them to the beneficiary provider or institution immediately and securely, and gives them to authorities on request.
- The beneficiary side holds the same data and makes it available.
- Monitoring, freezing and the ban on dealing with designated persons apply on the same basis.
- The same duties apply to financial institutions sending or receiving virtual assets for a customer.
- The data may be sent directly or indirectly and need not sit on the transfer.

Source, 2021 guidance, direct: transfers are treated as cross-border (¶179). Submission must be before or when the transfer happens, and sending after the fact should not be allowed; batches are acceptable if immediate and secure (¶187). Any technical solution is acceptable if it lets the parties comply (¶188). Transfers to unhosted wallets get a modified treatment (¶179). Inference: `REQ-ORD-*` is affected, because the required data must exist and move before or with the transfer stage, not after an order settles.

**Recommendation 16, as revised on 2025-06-18.** I could not read the text. Source, secondary:
- It is renamed "Payment transparency", and changes take effect by end-2030 ([A&O Shearman](https://finreg.aoshearman.com/fatf-updates-standards-on-payment-transparency)).
- The chain starts with the institution receiving the customer's instruction, and accountability for including and keeping information is clarified. Cross-border peer-to-peer above USD/EUR 1,000 carries the sender's name, address and date of birth. Institutions adopt tools such as recipient-detail verification.
- The explanatory note reportedly does not bring VASPs within Recommendation 16 itself. They stay under Recommendation 15, and the Interpretive Note to 15 will change only if needed, so bank and crypto data sets may diverge. Draft guidance consultation closed on 2026-08-21 ([law-firm wiki, 2026-09-28](https://wiki.private.law/en/travel-rule)).
- The Recommendation reportedly does not itself require real-time sanctions screening, per a footnote *(search excerpt, secondary)* ([commentary](https://fincrimeradar.org/fatf-recommendation-16-intelligence-brief.html)).
- An assessment annex followed in October 2025 *(search excerpt)* ([FATF page](https://www.fatf-gafi.org/en/publications/Fatfrecommendations/update-Recommendation-16-payment-transparency-june-2025.html)).

Inference: the starting point of that chain matters. If rutent receives the customer's instruction and passes it on, the question of who starts the chain is live. The answer depends on whether rutent is an institution at all.

**Enforcement in practice.** Source, from a vendor's summary of the 2026 targeted update: 83 percent of responding jurisdictions (91 of 109) have Travel Rule legislation, and 55 of those 91 have issued no findings or enforcement on it, as of April 2026, self-reported. The same post's summary line gives a figure near half, which conflicts ([vendor post, 2026-07-20](https://www.21analytics.co/blog/2026-fatf-targeted-update-part1/)). I could not read the FATF report.

**Records and sanctions.** Source, GAFILAT edition: Recommendation 11 sets at least five years for records. The Interpretive Note to Recommendation 15, ¶7(b), extends freezing and the ban on dealing with designated persons to these providers.

### Colombia (Bre-B)

- **No licensing regime that I could confirm.** Bill 510 of 2025, which would have created a provider registry, was shelved in 2026, and reports say four bills have failed since 2019 *(search excerpts)* ([La FM](https://www.lafm.com.co/economia/colombia-criptomonedas-regulacion-mercado-415573), [Asuntos Legales](https://www.asuntoslegales.com.co/consumidor/como-va-en-el-pais-toda-la-regulacion-de-las-criptomonedas-y-que-falta-por-avanzar-3883884)).
- **Reporting duty exists.** Source: UIAF Resolution 314 of 2021 applies to natural or legal persons who, for or on behalf of another and whatever the amount, perform exchange, transfer, custody or administration, issuer-related financial services, and more generally services related to virtual assets. It requires immediate suspicious-operation reports, monthly reports of nil suspicious activity, of transactions (individual from USD 150, multiple from USD 450), of nil transactions, and of all clients with their products. Sanctions run through the companies supervisor or the relevant supervisor ([text via compilation](https://normograma.com/keralty/compilacion/docs/resolucion_uiaf_0314_2021.htm)). The start date moved to 2022-07-01 *(search excerpt)*. The digest found no retention rule in that resolution.
- **Bre-B.** Source: Circular DSP-465, dated 2024-04-30 and amended 2024-09-20, sets standards for administrators of immediate low-value payment systems, including keys, clearing and messaging format ([Cuatrecasas](https://www.cuatrecasas.com/es/latam/art/cambios-interoperabilidad-sistemas-de-pago-bajo-valor-inmediatos)). A draft of 2026 adds rules for participants that do not offer payment instruments, and a decree proposal would redefine who may participate *(search excerpts)*. I found nothing on whether an unlicensed platform may participate, and could not read Banco de la República's own pages.
- **Companies supervisor.** Chapter X of its Basic Legal Circular sets the self-control regime for covered companies, with a July 2026 amendment I could not read *(search excerpts)* ([supervisor page](https://www.supersociedades.gov.co/en/web/asuntos-economicos-societarios/sagrilaft)).
- **Tax.** A low-quality source mentions a tax-authority resolution requiring provider reporting from tax year 2026 *(search excerpt)*. A lead only.
- Inference: Resolution 314's "services related to virtual assets" is very wide and has no control test, so an instructing platform cannot assume it is outside. Colombia's position is an unsettled law with a live reporting duty.

### Brazil (Pix)

**Numbering caution.** BCB's press note says Resolution 519 covers service provision and 520 covers authorisation. The document returned to me for "BCB 520" contains service, custody, AML and travel-rule articles. Law-firm summaries disagree among themselves *(search excerpts)*. Citations below are to the URL fetched, called "the fetched text". Check numbering before relying on it. One article on custody technology providers came back with different numbers across two digests, so I refer to it only as the custody article.

- **Scope.** Source, press note, direct: effective 2026-02-02, extending all customer-protection, AML, governance and security regulation to providers. Provider types are intermediaries, custodians and brokers ([press note](https://www.bcb.gov.br/detalhenoticia/20918/nota)).
- **Who may provide.** Source, fetched text: providers may do only what the resolution permits (Art. 5). Intermediation is activity for third parties (Art. 7). Operating in Brazil means being incorporated with head office there (Art. 1, sole paragraph), and foreign entities already operating had 270 days to move operations to a compliant Brazilian entity (Art. 23).
- **Technology suppliers.** Source, fetched text: technology specifically related to the provider's virtual-asset services counts as a relevant service (Art. 33). Before contracting, the provider must check the supplier's capacity, keep controls on sanctions lists and sanctioned wallet addresses, and tell clients about contracted entities (Art. 34). Contracted entities act on its behalf and under its direction (Art. 35). It answers for transactions through them (Art. 36). Contracts must give the central bank access to information and premises (Art. 40). A custody technology supplier must certify it cannot affect custody or client rights (the custody article). Inference: a mere technology supplier is not itself a provider on this text, which suits a `rutent` operator, but only if it really is one, and only with these duties flowing down.
- **Travel Rule.** Source, fetched text, Art. 44 sole paragraph: the sending provider gives the receiving institution the originator's name, full account identification, address, tax or equivalent identifier and wallet, and the beneficiary's name, account identification and wallet.
- **Records.** Source, fetched text, Art. 45: transaction records for at least five years. Source, [Circular 3,978, 2020, Art. 67](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Circular&p2=3978): at least ten years for customer data, transaction records and suspicion dossiers; five years for some governance documents (Art. 66). Inference: these cover different scopes or may stack. I did not settle which governs a given provider record.
- **Customer checks.** Source, Circular 3,978: identify and verify before starting a relationship (Arts. 16, 17, 23). Suspicious-activity analysis cannot be outsourced (Art. 44). For non-authorised third parties in shared payment arrangements, contracts must give access to end-recipient identities (Arts. 31, 60). The digest found no UN sanctions clause in the circular. I did not read the separate sanctions law.
- **Foreign exchange.** Source, [BCB 521](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=521): international payments with virtual assets, transfers to or from self-custody wallets, and buying, selling or swapping fiat-referenced assets are foreign-exchange activity (Art. 76-A). The provider must identify a self-custody wallet's owner and verify origin and destination (§5). Moving third-party funds through these services is barred, save for an authorised institution acting for its clients (§3). A local provider must check a foreign provider's supervision or document a risk assessment (Art. 76-B, §2). Monthly reporting began 2026-05-04 (Arts. 6, 82-A). International transfers are capped at USD 100,000 for these companies when the counterparty is outside the FX market (Art. 29). Inference: a stablecoin leg of a rutent order is plausibly FX in Brazil, and the wallet-owner duty pulls identity toward the on-chain side.
- **Non-authorised front ends.** Source, Joint Resolution 16: an unauthorised Brazilian company may act as the client-facing party in defined services, must tell clients it is not authorised, and supplies information for customer checks, while the authorised provider keeps responsibility and accounts sit in the client's own name (Arts. 3, 4, 8, 9, 10). Existing contracts must comply by 2026-12-31 (Art. 22). Data processing and cloud are outside its scope (Art. 3). Inference: this is the clearest example found of a legal shape for "instructing without being licensed", and its condition, one account per client, conflicts with pooled operation.
- **Pix access tightening.** A R$15,000 cap on non-authorised institutions and those connected through technology suppliers, and R$15 million capital for those suppliers, were reported in September 2025 *(search excerpts)* ([one report](https://www.congressoemfoco.com.br/amp/noticia/111658/bc-fixa-limite-de-r-15-mil-no-pix-para-instituicoes-nao-autorizadas)).

### Mexico (SPEI)

- **Financial-sector route.** Source: fintech institutions need Banco de México's authorisation for virtual-asset operations, and Circular 4/2019, according to one source, limits them to internal operations ([Library of Congress, updated 2020](https://wwws.loc.gov/law/help/cryptoassets/mexico.php); *search excerpts* for the limit). That summary is old.
- **Anti-money-laundering route.** Source: the July 2025 reform of the anti-money-laundering law, published 2025-07-16, keeps virtual-asset commerce as a vulnerable activity, considers operations with Mexican citizens from another jurisdiction, and requires ten-year retention ([Hogan Lovells](https://www.hlc.com/es/publications/reform-of-mexicos-antimoney-laundering-law)). Source, direct: the tax authority's 2026 FAQ says obliged parties are those who habitually or professionally offer exchange through platforms they administer or operate, facilitate or perform purchase or sale of assets owned by clients, or provide means to hold or transfer virtual assets other than those recognised by Banco de México. Foreign persons doing a vulnerable activity must comply and register ([FAQ 1.11, 2.8](https://www.sat.gob.mx/minisitio/ActividadesVulnerables/documentos/Preguntasfrecuentes2026.pdf)).
- **August 2026 rules.** Source: Acuerdo 115/2026, published 2026-08-07, generally effective 2026-11-30, with staged duties: risk approach and customer due diligence by 2027-03-01, automated monitoring by 2027-06-01, first annual audit period 2028 ([DOF](https://dof.gob.mx/nota_detalle.php?codigo=5795797&fecha=07/08/2026), [Hogan Lovells](https://hlc.com/es/publications/amendments-to-the-general-rules-under-the-aml-law-key-changes-and-implementation-timeline)). Registration requires shareholders, beneficial controller, trade name, websites or apps, and representatives, and existing registrants update within six months (DOF, Arts. 10 Bis to 10 Quáter; [Galicia note, 2026-08-20](https://www.galicia.com.mx/links/files/Actualizaciones/GC.-Actualizacion.-Regulatorio.-Modificaciones-a-las-reglas-de-prevencion-de-lavado-de-dinero.pdf)). Registration is not an authorisation (DOF, Art. 10 Quáter).
- **The critical reading.** Source, secondary, direct from the Galicia note: facilitation or intermediation exists where a provider supplies infrastructure, interfaces or electronic platforms that connect, reconcile or match buying, selling, exchange or custody of virtual assets, even if it does not control the assets or limits itself to intermediating flows in local or foreign currency. Thresholds are 210 times the daily UMA unit per operation, or 4 times that unit in fee. Notices must carry originator and recipient data: name, residence, account identifier, wallet, time, asset, amount, operation type and fee. I could not see this wording in the DOF text that the fetch tool returned, which ended mid-chapter; [Cuatrecasas](https://www.cuatrecasas.com/resources/actividades-vulnerables-text-6a7b0ea186983639164220.pdf) confirms specific virtual-asset rules on originator, recipient and traceability, and 24-hour suspicion notices (Art. 26 Bis 1), with documents kept ten years. Inference: if that reading is right, rutent's core function, connecting reserves, ramps and exchanges and intermediating local-money flows, sits inside the text for whoever operates it in Mexico. This is the single most consequential legal question in this report.
- **SPEI.** Source, direct: Circular 9/2026, published 2026-06-17, adds mobile user-experience guides to the SPEI rules ([Circular 9/2026](https://www.banxico.org.mx/marco-normativo/normativa-emitida-por-el-banco-de-mexico/circular-14-2017/%7B7D40F0A1-70F8-74D8-8F82-D96401557147%7D.pdf)). Direct and indirect participants must comply by 2026-12-14, and display of the recipient's masked name is reported *(search excerpts)*. I did not read Circular 14/2017's participant rules.
- **Sanctions list duties in Mexico.** I could not determine them for a non-financial provider.

### Who carries what

Reading, not settled. "Instructs" means the operator never holds value and only passes orders to licensed providers. "Holds" means the operator controls value or keys at any point. `OQ-10` decides which column applies.

| Duty | Instructs only: where it probably sits | Holds value: where it probably sits | What could move it |
|---|---|---|---|
| Customer identification | Licensed provider (all texts read) | Operator, also | Mexico's facilitation reading; a VASP finding by function |
| Travel Rule data | Sending and receiving provider or institution | Operator, as sending or receiving provider | Whether the operator takes the customer's instruction (`R.16` chain) |
| Sanctions screening | Provider. Brazil's text also requires controls on lists and addresses when contracting | Operator, also | A provider contract; rail rules |
| Suspicious-activity reporting | Provider; cannot be outsourced in Colombia or Brazil | Operator, also | Same functional tests. Colombia's Resolution 314 has no control test |
| Registration or licence | Provider. Possibly the operator in Mexico (registration) and Colombia (reporting) | Operator: Brazil licence and domestic incorporation; Mexico registration, and authorisation if a financial entity | Deployment country, marketing to those residents |
| Retention | Provider, at 5 to 10 years. Operator by contract, and by Mexican rules if captured | Operator: plan for at least 10 years | The longest applicable rule |
| Outsourcing evidence | Falls on the provider, and reaches the operator as supplier | Operator, for its own suppliers | Provider's regulator |
| Open-source project itself | Probably none, while it only publishes code (¶76, ¶82) | Same | Keeping control, running a hosted service, or holding signing authority over what runs |

### What triggers `reg-fintech`-type duties

The owner ruled to add that profile "if regulation demands" (`PS-60`, R8). The profile candidate means `oss-ledger` plus a PCI DSS and SOC 2 evidence map, a third-party register, and validation independence for models.

Inference, from the readings above. Three distinct things could count as regulation demanding it:
1. **The operator is itself captured.** This is a VASP finding by function, Mexican registration, Brazilian authorisation, or holding value (`OQ-10`).
2. **The operator is a supplier to someone captured.** Source: under the fetched text (Arts. 32 to 36, 40) and [BCB 85, as amended to 2026-03-03](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=85) (due diligence covering certifications, access to independent audit reports and data, Art. 12; continued responsibility, Art. 14; notice to the central bank within ten days, Art. 15; foreign-provider conditions including an information-exchange agreement, Art. 16). This can apply even where rutent is unlicensed, and even for a foreign operator a licensed provider hires.
3. **The project publishes something that operators in (1) or (2) must rely on.** That demand reaches the project only through what operators ask of it.

Mapped to the profile's four elements:
- **Third-party register.** Most directly supported. It mirrors what the Brazilian texts expect of a provider and what `PS-14`'s catalogue already nearly is.
- **SOC 2.** I found no law in the three countries naming it. Art. 12 of BCB 85 makes audit reports and certifications something a provider must check, so it can arrive commercially.
- **PCI DSS.** Relevant only if cards or card data enter scope. `PS-1`'s rails are not cards. I did not research PCI further.
- **Model validation independence.** No text I read requires it of a platform in this position. Mexico's rules require automated monitoring and risk models of those captured, per Hogan Lovells. Whether the quoting engine counts as a model in any regulator's sense, I could not determine.

So the honest answer to "does regulation demand it" is that it depends on `OQ-10`, on deployment jurisdiction, and on counsel's reading of the Mexican text. It is more likely than the phrase "if regulation demands" suggests, because the second route needs no licence.

## 4. ISO 20022

`profile.oss-ledger.md` leaves open whether a mapping is relevant (`oss-ledger-3`).

**Rails.**
- **Pix.** Source: BCB's service catalogue, version 5.11, describes ISO 20022 messages in the instant payment infrastructure, XML with schema validation, with pacs.008 and pacs.002 forming the basic cycle and pacs.004 for returns, and camt.052 for position reports in some flows *(search excerpts, from bcb.gov.br documents)* ([catalogue](https://www.bcb.gov.br/content/estabilidadefinanceira/cedsfn/Catalogos/Catalogo_de_Servicos_do_SFN_Volume_VI_Versao_511.pdf)).
- **SPEI.** Not established. A 2021 slide from Banco de México reportedly said no immediate plan to migrate, while a low-quality guide says it supports it *(search excerpts)* ([slides, unreachable](https://clon.cemla.org/actividades/2021-final/2021-09-ii-reunion-responsables-infraestructuras-de-mercados-financieros/4b.%20Moreno_CEMLA%20FPS%20Mexico%20SPEI%20BdeM.pdf)).
- **Bre-B.** Not established. DSP-465 sets a messaging format, but the sources I could read do not name a standard. ISO 20022 documents found on Banco de la República's site concern the securities depository *(search excerpt)*.

**Cross-border direction.** Source: CPMI published harmonised ISO 20022 data requirements on 2023-10-17, with alignment encouraged by end-2027 ([BIS](https://www.bis.org/cpmi/publ/d218.pdf)). Source: FATF's 2021 guidance accepts any technical solution for the Travel Rule (¶188). Source, weak: ISO 20022 can carry fiat-leg data alongside a separate Travel Rule payload for the virtual-asset leg, per a blockchain analytics vendor *(search excerpt)*. A claim that FATF names ISO 20022 as preferred for VASPs appears only on one low-authority glossary and I discount it.

**Where it could apply.** Inference. rutent would not normally be a participant exchanging these messages on a rail. Its providers would. So a wire-level obligation seems unlikely at rutent's own boundary. What remains is a semantic mapping, plausibly useful in three places:

| rutent concept (pending in `SPEC.md`) | Nearest ISO 20022 message | Basis |
|---|---|---|
| A customer's request to pay out | pain.001 | State Street, BNY guides *(search excerpts)* |
| A stage's execution on a rail | pacs.008 | BCB, BNY *(search excerpts)* |
| Status of a stage | pacs.002 | BCB *(search excerpt)* |
| Return of funds | pacs.004 | BCB, BNY *(search excerpts)* |
| Compensation requested | camt.056; answer by camt.029; positive result by pacs.004 | [BNY module](https://www.bny.com/content/dam/bnymellon/documents/pdf/iso-20022/presentation-module-6.pdf), [CPMI](https://www.bis.org/cpmi/publ/d218.pdf) *(search excerpts)* |
| Account or name check | acmt.023 and its report acmt.024 | [BIS Nexus](https://docs.bis.org/nexus/messaging-and-translation/message-acmt.023-identification-verification-request), [ISO draft](https://www.iso20022.org/sites/default/files/2020-12/ISO20022_MDRPart2_ChangeOrVerifyAccountIdentification_2020_2021_v1_ForSEGReview.pdf) *(search excerpts)* |
| Reconciliation against a provider statement | camt.053, camt.054 | Inference, unverified: from recall, not checked against any source in this session |
| Status inquiry for an unknown outcome | pacs.028 | Inference, unverified: from recall, not checked against any source in this session |
| Quotes, routes, reserve yes-or-no checks, on-chain legs | None obvious | Inference. I did not search the catalogue for them |

Inference: the most valuable overlap may be vocabulary, not messages. A mapping tests whether rutent's outcome and compensation states mean what the banking world thinks they mean, for instance whether a request to return funds is distinct from the return itself, as `research/2026-11-02-durable-state.md` already argues for rails. A mapping for `PS-21`'s quotes and routes looks unjustified on what I found.

**Edition.** The profile says to pin an edition when citing. Message versions change on a yearly cycle *(search excerpt)*, and I could not confirm the current ISO 20022 part editions, so no pin is proposed here.

**Is it relevant?** That is for the spec authors and owner, not this report. Facts bearing on it: one rail of three is confirmed on it, the others unknown; rutent is not a rail participant by default; and an informative mapping costs little, whereas a normative one would need all three rails established first.

## 5. Abuse

Each maps to `SPEC.md` threat areas. Evidence is cited; the "limit" column is Inference.

| Abuse | How it works | Evidence | Limit of what can be done |
|---|---|---|---|
| False attestation | A provider says yes without checking, or to win volume | UK bank above: false in effect for years, though not deliberately | Undetectable by rutent. Accountability only |
| Stale attestation | A check once true outlives the facts: sanctions listing, licence loss, closed account | Bitstring and status drafts set no validity period; nothing forces a recheck | Short validity raises load and leaks timing. Long validity raises risk |
| Fronting | One verified party acts for others. Mules, rented accounts, pooled accounts | FATF's February 2026 paper mentions trade in bank and exchange accounts, nominee accounts and mule networks *(search excerpt)* ([FATF page](https://www.fatf-gafi.org/en/publications/Methodsandtrends/cyber-enabled-fraud-digitalisation-ml-tf-pf-risks.)). Brazil's BCB 518 and 521 §3 target it | Key binding proves possession of a key, not who benefits. A verified human running a hundred agents (`PS-2`) looks the same |
| Credential sharing and replay | A bearer statement reused by someone else | OpenID4VP 14.1.1: without holder binding the verifier takes the risk | Binding moves the problem to key custody and to who controls the device |
| Colluding providers | A reserve vouches for users a colluding ramp or exchange needs; a provider registers twice | No fintech-specific evidence found | Independent rail or chain records can expose it, if someone compares them |
| Compromised or malicious module | A module signed by a provider returns yes, whatever the provider's real view | Inference from `PS-18` and `PS-20` | Tie the assertion to the provider and module version so blame is clear |
| Correlation across verifiers | Same issuer signature, key or status index seen in two places | RFC 9901 §10.1, OpenID4VP §15.5, Bitstring §6.6, §6.7 | Batch issuance helps verifier-verifier only. Issuer-verifier collusion cannot be stopped in salted-hash schemes. Small populations defeat group privacy |
| rutent as correlator | Being the common verifier, rutent sees one user's activity across providers if any reference repeats | Inference | Per-provider references help, and then the cross-provider abuse in section 2 cannot be detected. This is a direct trade-off |
| Issuer tracking by status check | The issuer learns who checks and when | Bitstring §6, RFC 9901 §10.1 | Caching lowers freshness |
| Inference from yes-or-no checks | Binary search on "has at least X?" | See below | See below |
| Check-then-act | A yes is not a reservation | Durable-state report, section 2 | Existence of funds at check time proves nothing later |
| Regulator-facing misuse | A proof treated as if it discharged a duty | Section 2 | Language in the guarantees and obligations split |

### Inference from repeated checks

Measured: Tikhomirov, Pickhardt, Biryukov and Nowostawski recovered Lightning channel balances by sending fake payments. It takes under a minute per channel, needs moderate capital and no expenditure, and was demonstrated on testnet ([arXiv 2004.00333, 2020](https://arxiv.org/abs/2004.00333)). The authors describe learning a balance to arbitrary precision by binary search *(search excerpt, their mailing-list post)*. Source: naive probing can mislead on parallel channels, and jamming-enhanced probing recovers those too *(search excerpt)* ([Biryukov et al.](https://ia.cr/2021/384)). Their countermeasure, hiding which channel failed, costs routing reliability.

Source: Brazil requires Pix participants to keep mechanisms against read attacks on the key directory, at least as strong as the directory's own, and a procedure for excessive queries that do not settle and for keys not registered (Arts. 84-A and 84-B of the Pix regulation, inserted by [BCB 147, 2021-09-28](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=147)). A participant's developer documentation describes the mechanism as a token bucket, in which failed lookups cost extra once a threshold passes, and a central-bank forum presentation lists requester identity checks and comparing queries with orders sent *(search excerpts)* ([forum](https://www.bcb.gov.br/content/estabilidadefinanceira/pix/Forum_Pix_Plenaria/20221201-Forum_Pix.pdf)). This is a real rail's regulator treating lookups as an attack surface.

Inference:
- Binary search over a range of a trillion units needs about 40 yes-or-no answers. If the threshold is chosen by the asker and answers are free and unlinked, `PS-31`'s promise fails against anyone allowed to ask.
- A yes-or-no answer also reveals that the account exists at that provider, and timing may reveal more. Neither is a balance, and both may matter under `PS-30`.
- What narrows this: who may ask, about whom, how often, whether the threshold must match a real order, whether the question costs something, whether repeated questions are visible to the provider or subject, and whether answers are quantised or delayed. Each one has a usability price. Differential-privacy style noise is possible in principle, but a wrong yes on a payment check has real cost. I did not research this.
- Because the check runs inside a provider module (`PS-19`, `PS-20`), who enforces limits, the module or the platform, is a design decision to leave open at spec level.

## 6. What this implies

### 6a. Capabilities to think about stating

None names a mechanism; each could be met in more than one way. These are prompts for the spec authors, not requirements.

**The statement itself** (`REQ-CPL-*`, from `PS-22`)
- An eligibility statement identifies who makes it, about which subject reference, for which set of transactions, in which jurisdiction, when it was made, and how long it may be relied on.
- A statement covers a stated scope. It does not imply anything outside it.
- "No", "not known" and "not asked" are distinct outcomes.
- The maker cannot later deny having made a statement, and others cannot fabricate one in its name.
- A statement can be withdrawn, within a declared time bound, with defined behaviour for orders in flight.
- A statement is bound to the request or order it supports, so it cannot be reused for another.
- rutent's records keep "the provider said" apart from anything rutent verified itself (with `REQ-AUD-*`).

**What it may not carry** (`PS-30`, `PS-31`)
- rutent's need for identity data is stated per class of transaction, and by default is none beyond a reference.
- A person with a lawful right can get from the responsible provider the evidence behind a statement, given a reference, without rutent holding that evidence.
- References are such that correlation across providers is a deliberate, stated choice, either way.

**Actors other than end users** (`REQ-CPL-*`, `REQ-CAT-*`)
- A provider's own standing (legal identity, licence or registration, jurisdictions, who can act for it) is declared, evidenced and given an expiry, and its loss changes what it may do.
- Whoever posts a quote, route or module can be tied to an accountable legal party (`PS-21`).
- An agent's authority traces to an accountable principal and a stated scope (`PS-2`).

**Authorisation** (`REQ-AUZ-*`, `PS-13`)
- An eligibility statement is an input to rutent's policy decision, not the decision. Policy can refuse a yes.
- The policy version and inputs of each decision are recorded.
- External authentication proves who is calling, and is not taken as eligibility.
- An operator or lawful authority's hold or block can be applied and is recorded.

**Routing and data flow** (`REQ-QTE-*`, `REQ-ORD-*`, `REQ-CAT-*`)
- Routes consider each provider's declared jurisdiction and compliance capabilities, and refuse a stage that cannot lawfully be served.
- Where a law requires originator and beneficiary data to travel, it can reach the next party before or with the stage, whether or not rutent can read it.
- End users are represented individually through an order, so a provider need not pool clients to use rutent.
- Each stage can show what the licensed party at that stage knew and did.

**Balance verification** (`REQ-CAT-*`, invariant stub, `PS-31`)
- Who may ask, about whom, how often and at what thresholds is bounded and stated.
- Repeated checks can be noticed by a party able to act on them.
- A check is not a reservation.

**Evidence and retention** (`REQ-AUD-*`)
- Statements and the decisions they fed are provable to a chosen third party, with disclosure limited to what is needed.
- Retention can be configured to the longest period that applies, ten years being the longest seen here, and retention and erasure duties can both be met.
- A supervisor's lawful access to an operator's records can be served without changing the system.

**Project versus operator** (`GUA-*`, `OBL-*`, `oss-ledger-2`)
- rutent's guarantees do not include legal compliance. What an operator must do per jurisdiction is listed as an operator obligation.
- The project's outputs do not describe a statement as discharging anyone's legal duty.
- An operator can restrict features by jurisdiction, including ones that may change its regulatory status, for example order matching (`PS-23`).
- Evidence a licensed provider needs about its supplier can be produced by an operator.

**Threat model** (`SPEC.md` section 8)
- "False compliance proofs" is stated with its accepted residual: rutent cannot detect a false statement. The mitigation is attribution and consequence.
- Add areas not yet listed: correlation by rutent itself, fronting, and collusion between providers.

### 6b. Candidate mechanisms for non-binding suggestions

For `suggestions/`. No ranking is implied. Facts are sourced above; trade-offs are Inference.

**Statement format**

| Candidate | For | Against |
|---|---|---|
| Provider-signed statement in a simple signed-token format, with a contract behind it | Smallest thing that gives attribution; fits "reserve proves"; no wallet needed; works in any module language | No selective disclosure; no standard vocabulary; linkable; revocation must be designed |
| W3C VC 2.0 data model | Recommendation; several securing options; vocabulary room for agents and entities | Broad spec, so interoperability depends on a profile chosen; correlation is left to the deployer |
| SD-JWT VC | RFC base and a near-final profile; works with common signature keys | Linkable unless batch issued; issuer-verifier collusion cannot be prevented; draft status for the profile |
| BBS | Unlinkable presentations; pseudonym option | Draft cryptography, features at risk; pairing curve and not post-quantum; disclosed fields can still identify |
| Zero-knowledge over an existing signed credential | No issuer change; strong minimisation | Requires a holder and device; no regulator acceptance found; library described by a third party as experimental |
| Published chain attestations | Openly verifiable; no status server | Permanent public link between wallet and verified status, which is awkward with `PS-30`; correction is hard |

**Presentation and status**

| Candidate | For | Against |
|---|---|---|
| OpenID4VP | Final; replay binding; multi-format | Only useful if end users hold credentials; adds a wallet flow for agents |
| Provider-to-platform call with signed response | Fits `PS-22` and module design; no user step | The provider sees every check; correlation sits with rutent |
| Status list (Bitstring or Token Status List) | Standard; caching hides checks | Small populations undermine privacy; freshness versus leakage |
| Short-lived statements with no revocation | Simple; no status server; no tracking callback | Issuance load; a window of staleness is accepted |

**Provider and agent identity**

| Candidate | For | Against |
|---|---|---|
| Verifiable LEI | Governed root; roles for people; legal-entity anchor | Issuer dependency and cost; I did not check how widely it is used in the region |
| Plain certificates plus a public register check | Familiar; simple | Register availability varies by country; I did not check any register |
| Agent mandate credentials | Directly on point for `PS-2` | Young, formats conflict across sources, no regulator view found |

**Inference control** (`PS-31`)

| Candidate | For | Against |
|---|---|---|
| Per-requester budget, as a token bucket | Precedent on a real rail | Sybil requesters; tuning; hurts honest bursts |
| Check only in the context of a real order and its amount | Closes free threshold choice | Orders can be created and dropped cheaply unless that has a cost |
| Fixed coarse thresholds | Bounds information per subject | Poor fit if amounts are precise; rounding errors at the edge |
| Telling the provider, or the subject, about patterns | Uses the party with the duty | Tipping-off rules may apply to subjects; I did not research them |

**Regulated structure**

| Candidate | For | Against |
|---|---|---|
| Stay a technology supplier to licensed providers | Fits the "instructs only" intent | Mexico's reading may defeat it; duties flow down by contract; needs real absence of control |
| Licensed front-end arrangement, on the Brazilian BaaS shape | Clear allocation; explicit law | Brazil-only; accounts per client; does not cover virtual assets on its face |
| Operator takes its own licence or registration per country | Removes ambiguity | Cost; contradicts a thin platform; Brazil needs domestic incorporation |
| Separate legal operator per country, under one open-source project | Keeps the project out | Governance overhead; depends on `PS-12` and `oq-product-map` |

**Standards alignment**

| Candidate | For | Against |
|---|---|---|
| Informative ISO 20022 mapping of states and messages | Cheap; tests vocabulary; satisfies `oss-ledger-3` if judged relevant | Only one rail confirmed; may date quickly |
| Normative ISO 20022 fields in the provider interface | Reduces translation at the provider | Pushes a banking model on on-chain and reserve-check parts; not supported by what was found |
| A Travel Rule interchange format and protocol | Addresses an actual duty for VASP-side providers | Belongs to providers, and I did not research current protocols or adoption |

## 7. Questions only the owner can answer

- Does rutent, or its operator, ever hold value or keys (`OQ-10`)? Most of section 3 turns on this.
- In which countries will rutent first operate, and will a Mexican deployment be in v1, given the August 2026 rules?
- Will you take legal opinions in Colombia, Brazil and Mexico before `SPEC.md` assumes a regulatory position?
- Does "provable at compliance level" mean provable to the operator, to providers, to a named supervisor, or to any third party?
- Is rutent content with proving that a provider said something, and accepting that it cannot prove the statement is true?
- Who bears the consequence of a false statement from a provider: the provider alone, the operator, or the user whose order is unwound?
- What happens to a provider caught lying: removal, a financial stake, or nothing beyond contract?
- Must a provider hold a licence somewhere to join the catalogue (`OQ-13`), and is that checked once or repeatedly?
- Will end users ever hold a credential of their own, or does the reserve always speak for them, as `PS-22` reads?
- May a reference to one user be the same across providers? Yes helps abuse detection; no fits `PS-30`.
- Is rutent willing to hold any personal data at all, if the law requires it for a deployment?
- Who may run a balance check, at what thresholds, and may it be asked outside a real order?
- May a provider pool end users behind one account when using rutent, given Brazil's rule against concealing third parties?
- For agents: who is the accountable party, and may one person's verified status cover many agents?
- If the operator is captured in one country, may features be turned off there, for example order matching?
- Is `reg-fintech` wanted now on the supplier route of section 3, or only if the operator itself is regulated?
- Is an informative ISO 20022 mapping enough for `oss-ledger-3`, or should the provider interface use it?
- How long must rutent keep compliance evidence, and does any erasure duty you accept override that?
- Does rutent's project itself keep any signing or control role over what runs in an operator's deployment? That affects ¶67 and ¶82.

## 8. What I could not determine

- The current English text of the FATF Recommendations, Recommendation 17's present wording, the revised Recommendation 16 and its Interpretive Note, the explanatory note, the FATF's draft guidance, and the 2026 targeted update. The FATF site was closed to my tools.
- Whether the Mexican facilitation language appears as the law-firm note says. It was not in the portion of the DOF text I could get.
- Whether any non-custodial platform has been treated by a supervisor as a provider or as outside the definition in Colombia, Brazil or Mexico. I found no decision.
- Brazil: which of BCB 519 and 520 holds which article, given the conflict above. Also the current sanctions law's reach over a technology supplier, and whether a foreign technology supplier may serve a licensed provider once Art. 16 of BCB 85 is applied.
- Colombia: document-retention period for a provider under Resolution 314, sanctions duties for an unsupervised platform, the current Chapter X and whether it names providers, the state of the 2026 Bre-B drafts, and Bre-B's messaging standard. Banco de la República's site was closed to my tools.
- Mexico: Circular 14/2017's participant rules, Circular 4/2019's current scope, sanctions duties for a non-financial provider, and how Ley Fintech and the anti-money-laundering law divide a platform that only intermediates local money.
- Whether SPEI uses ISO 20022 today, and the current ISO 20022 part editions and IVMS 101 version.
- Any regulator in the three countries accepting a zero-knowledge proof or a third party's reusable credential as due diligence.
- Production use of VC 2.0, SD-JWT, BBS or OpenID4VP in regulated finance. The EU wallet mandate is the only deployment context found, and only from search excerpts.
- The camt.053, camt.054 and pacs.028 rows of the ISO 20022 table, which are from recall and unverified.
- Real rates of false or stale attestation. I found no measurement.
- Actual use of agent mandate credentials, or any regulator's position on agents as customers.
- National KYC utilities as a model, from primary sources. Results were vendor blogs.
- Any published case of a rail or regulator treating repeated balance checks as an attack, beyond the Pix directory analogue and Lightning research.
- Anything from Bre-B or SPEI equivalent to Pix's read-attack rule.
- Whether SD-JWT VC clears IESG on 2026-11-05.
- Data-protection laws in the three countries, which `PS-30` will meet and I did not research.
- Tipping-off rules as they would apply to telling a subject about repeated checks.

## References

All accessed 2026-11-02. Version or date as shown by the source; "undated" means none was visible.

**Credentials and proofs**
- [W3C Verifiable Credentials Data Model 2.0, Recommendation, 2025-05-15](https://www.w3.org/TR/vc-data-model-2.0/).
- [W3C Bitstring Status List 1.0, Recommendation, 2025-05-15](https://www.w3.org/TR/vc-bitstring-status-list/).
- [W3C Data Integrity BBS Cryptosuites, Candidate Recommendation Draft, 2026-09-10](https://www.w3.org/TR/vc-di-bbs/).
- [RFC 9901, SD-JWT, November 2025](https://www.rfc-editor.org/rfc/rfc9901.html).
- [draft-ietf-oauth-sd-jwt-vc-23, 2026-10-31](https://datatracker.ietf.org/doc/draft-ietf-oauth-sd-jwt-vc/). [draft-ietf-oauth-status-list-21, 2026-06-21](https://datatracker.ietf.org/doc/draft-ietf-oauth-status-list/). [draft-irtf-cfrg-bbs-signatures-14, 2026-10-29](https://datatracker.ietf.org/doc/draft-irtf-cfrg-bbs-signatures/).
- [OpenID for Verifiable Presentations 1.0, Final, 2025-07-09](https://openid.net/specs/openid-4-verifiable-presentations-1_0.html).
- [Frigo and shelat, eprint 2024/2010](https://eprint.iacr.org/2024/2010) *(search excerpt)*. [Technology company's post on open-sourcing, 2025-07-03](https://blog.google/technology/safety-security/opening-up-zero-knowledge-proof-technology-to-promote-privacy-in-age-assurance/) *(search excerpt)*. [zkKYC, eprint 2022/321](https://eprint.iacr.org/2022/321) *(search excerpt)*.
- [GLEIF, Introducing the vLEI; framework v4.0, 2026-03-25](https://www.gleif.org/en/vlei/introducing-the-verifiable-lei-vlei).
- Published-chain attestation by a large exchange and agent mandate credentials: search excerpts only, vendor and news pages, not listed as references.

**Reliance and enforcement**
- [FATF Recommendations, GAFILAT Spanish edition, © 2018, hosted by Honduras's banking commission](https://pplaft.cnbs.gob.hn/wp-content/uploads/2020/10/40-Recomendaciones-Actualizacion-Dec-2019.pdf). Read directly for Recommendations 11, 15 and 17 and the Interpretive Notes to 15 and 17.
- [FATF Updated Guidance on Virtual Assets and VASPs, Spanish, October 2021, hosted by Mexico's tax authority](https://www.sat.gob.mx/minisitio/ActividadesVulnerables/documentos/Normateca/Internacional/EnfoqueBasadoenRiesgo_activosvirtuales.pdf). Read directly for ¶¶55, 56, 60, 63, 67, 70, 75, 76, 81, 82, 178-180, 187, 188, 205, 206.
- [EU AML Regulation 2024/1624, Art. 48](https://www.springlex.eu/en/packages/aml/amlr-regulation/article-48/), via a third-party rendering of the Official Journal text. [UK Money Laundering Regulations 2017, reg. 39](https://www.legislation.gov.uk/uksi/2017/692/regulation/39).
- [FCA final notice to a UK bank, 2024-09-27](https://fca.org.uk/publication/final-notices/starling-bank-limited-2024.pdf), pages 1 to 8.
- [US bank-fintech order summary, 2024-04-09](https://www.consumerfinancemonitor.com/2024/04/09/recent-fdic-consent-orders-show-increased-regulation-scrutiny-of-bank-relationships-with-fintech-partners/) *(search excerpt)*. [One bank's FY2024 annual report](https://www.sec.gov/Archives/edgar/data/842717/000095017025036301/brbs-20241231.htm) *(search excerpt)*.

**FATF, secondary**
- [A&O Shearman on revised Recommendation 16](https://finreg.aoshearman.com/fatf-updates-standards-on-payment-transparency). [Private.law wiki, Travel Rule, 2026-09-28](https://wiki.private.law/en/travel-rule). [Fincrime commentary](https://fincrimeradar.org/fatf-recommendation-16-intelligence-brief.html) *(search excerpt)*. [FATF update page, unreachable](https://www.fatf-gafi.org/en/publications/Fatfrecommendations/update-Recommendation-16-payment-transparency-june-2025.html) *(search excerpt)*. [FATF 2026 targeted update page, unreachable](https://www.fatf-gafi.org/en/publications/Fatfrecommendations/targeted-updated-virtualassets-vasps-2026.html) *(search excerpt)*.
- [Vendor summary of the 2026 targeted update, 2026-07-20](https://www.21analytics.co/blog/2026-fatf-targeted-update-part1/). [FATF cyber-enabled fraud paper, February 2026, unreachable](https://www.fatf-gafi.org/en/publications/Methodsandtrends/cyber-enabled-fraud-digitalisation-ml-tf-pf-risks.) *(search excerpt)*.

**Brazil** (BCB API returns the consolidated text as served on the access date)
- [Press note, 2025-11-10](https://www.bcb.gov.br/detalhenoticia/20918/nota). [Text fetched as Resolution BCB 520, 2025-11-10](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=520), first 200,000 of 302,000 characters. [BCB 521, 2025-11-10](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=521). [BCB 518, 2025-11-03](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=518). [BCB 147, 2021-09-28](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=147). [BCB 85, 2021-04-08, as amended to 2026-03-03](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20BCB&p2=85), first 100,000 characters. [Joint Resolution 16, 2025-11-28](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Resolu%C3%A7%C3%A3o%20Conjunta&p2=16). [Circular 3,978, 2020-01-23](https://www.bcb.gov.br/api/conteudo/app/normativos/exibenormativo?p1=Circular&p2=3978).
- [Pix forum, 2022-12-01](https://www.bcb.gov.br/content/estabilidadefinanceira/pix/Forum_Pix_Plenaria/20221201-Forum_Pix.pdf) *(search excerpt)*. [SFN service catalogue vol. VI, v5.11](https://www.bcb.gov.br/content/estabilidadefinanceira/cedsfn/Catalogos/Catalogo_de_Servicos_do_SFN_Volume_VI_Versao_511.pdf) *(search excerpt)*. [Conjur, 2025-09-18](https://www.conjur.com.br/2025-set-18/brechas-regulatorias-fintechs-e-o-desafio-do-combate-a-lavagem-de-dinheiro//?print=1) *(search excerpt)*. [Congresso em Foco](https://www.congressoemfoco.com.br/amp/noticia/111658/bc-fixa-limite-de-r-15-mil-no-pix-para-instituicoes-nao-autorizadas) *(search excerpt)*.

**Colombia**
- [UIAF Resolution 314, 2021-12-15, via a compilation](https://normograma.com/keralty/compilacion/docs/resolucion_uiaf_0314_2021.htm). [Superfinanciera Boletín Jurídico 116, concept of 2025-05-29](https://www.superfinanciera.gov.co/publicaciones/10115675/boletin-juridico-no-116/). [Cuatrecasas on DSP-465, 2024-09-24](https://www.cuatrecasas.com/es/latam/art/cambios-interoperabilidad-sistemas-de-pago-bajo-valor-inmediatos).
- [Supersociedades SAGRILAFT page](https://www.supersociedades.gov.co/en/web/asuntos-economicos-societarios/sagrilaft) *(search excerpt)*. [La FM](https://www.lafm.com.co/economia/colombia-criptomonedas-regulacion-mercado-415573) *(search excerpt)*. [Asuntos Legales](https://www.asuntoslegales.com.co/consumidor/como-va-en-el-pais-toda-la-regulacion-de-las-criptomonedas-y-que-falta-por-avanzar-3883884) *(search excerpt)*.

**Mexico**
- [Acuerdo 115/2026, DOF, 2026-08-07](https://dof.gob.mx/nota_detalle.php?codigo=5795797&fecha=07/08/2026), portion returned by the tool. [Galicia note, 2026-08-20](https://www.galicia.com.mx/links/files/Actualizaciones/GC.-Actualizacion.-Regulatorio.-Modificaciones-a-las-reglas-de-prevencion-de-lavado-de-dinero.pdf). [Cuatrecasas Legal Flash, August 2026](https://www.cuatrecasas.com/resources/actividades-vulnerables-text-6a7b0ea186983639164220.pdf). [Hogan Lovells on the 2025 reform](https://www.hlc.com/es/publications/reform-of-mexicos-antimoney-laundering-law). [Hogan Lovells on the 2026 rules](https://hlc.com/es/publications/amendments-to-the-general-rules-under-the-aml-law-key-changes-and-implementation-timeline).
- [SAT FAQ 2026](https://www.sat.gob.mx/minisitio/ActividadesVulnerables/documentos/Preguntasfrecuentes2026.pdf). [Banco de México Circular 9/2026, DOF 2026-06-17](https://www.banxico.org.mx/marco-normativo/normativa-emitida-por-el-banco-de-mexico/circular-14-2017/%7B7D40F0A1-70F8-74D8-8F82-D96401557147%7D.pdf). [Library of Congress, updated 2020](https://wwws.loc.gov/law/help/cryptoassets/mexico.php).

**ISO 20022**
- [CPMI harmonised data requirements, 2023-10-17](https://www.bis.org/cpmi/publ/d218.pdf). [BIS Nexus, acmt.023](https://docs.bis.org/nexus/messaging-and-translation/message-acmt.023-identification-verification-request) *(search excerpt)*. [ISO 20022 draft, change or verify account identification, 2020-2021](https://www.iso20022.org/sites/default/files/2020-12/ISO20022_MDRPart2_ChangeOrVerifyAccountIdentification_2020_2021_v1_ForSEGReview.pdf) *(search excerpt)*. [BNY module 6](https://www.bny.com/content/dam/bnymellon/documents/pdf/iso-20022/presentation-module-6.pdf) *(search excerpt)*. [CEMLA slides, 2021, unreachable](https://clon.cemla.org/actividades/2021-final/2021-09-ii-reunion-responsables-infraestructuras-de-mercados-financieros/4b.%20Moreno_CEMLA%20FPS%20Mexico%20SPEI%20BdeM.pdf) *(search excerpt)*.

**Inference attacks**
- [Tikhomirov, Pickhardt, Biryukov, Nowostawski, arXiv 2004.00333, 2020](https://arxiv.org/abs/2004.00333). [Biryukov, Naumenko, Tikhomirov, eprint 2021/384](https://ia.cr/2021/384) *(search excerpt)*.

**Other project reports**
- `research/2026-11-02-durable-state.md`, section 3 for audit evidence.

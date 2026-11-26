# `T-025-05`: every `DEC-20` path in the diff, with its verdict

The paths `git diff --name-only fb883e01..0e3f68db` lists under a `DEC-20` prefix (`HOUSE-RULES.md`, `.github/CODEOWNERS`),
with `components/prov/src/module/` whole, `tools/boundary-check/` and `conformance/evidence/v0-scope`, which `T-025-05` adds.
Merge base `fb883e01` (`origin/develop`), head `0e3f68db` (`m3`). Ids in a verdict are the findings and residuals of
`notes.md`, section `T-025-05`. "data" is a fixture: JSON that `contracts/tests/` reads valid or refused; none holds a
private key, a seed or a token (searched). Counts are lines added and removed.

| Path | + | - | Verdict |
|---|---|---|---|
| `.github/workflows/ci.yml` | 58 | 3 | sound; low L4 (jail step without `RUTENT_RELEASE_PROVIDER`) |
| `.github/workflows/jail-probes.yml` | 70 | 0 | sound |
| `.github/workflows/stm-vos.yml` | 61 | 3 | sound |
| `components/core-audit/Cargo.toml` | 21 | 0 | sound (no new crate of ours; boundary holds) |
| `components/core-audit/src/bin/audit-check.rs` | 204 | 0 | sound |
| `components/core-audit/src/checkpoint/checker.rs` | 344 | 0 | sound (`Missing` finds a skipped tick) |
| `components/core-audit/src/checkpoint/mod.rs` | 441 | 0 | sound; residual R-CURSOR (unsigned cursor) |
| `components/core-audit/src/compare/detect.rs` | 296 | 0 | sound |
| `components/core-audit/src/compare/evidenced.rs` | 81 | 0 | sound |
| `components/core-audit/src/compare/holding.rs` | 101 | 0 | sound |
| `components/core-audit/src/compare/mod.rs` | 42 | 0 | sound |
| `components/core-audit/src/compare/report.rs` | 76 | 0 | sound |
| `components/core-audit/src/lib.rs` | 15 | 0 | sound |
| `components/core-audit/src/log/kept.rs` | 92 | 0 | sound |
| `components/core-audit/src/log/keys.rs` | 587 | 0 | sound (register fed only by the log and settings); residuals R-OPMODULE, R-BACKDATE |
| `components/core-audit/src/log/merkle.rs` | 155 | 0 | sound |
| `components/core-audit/src/log/mod.rs` | 615 | 0 | sound (handle check before the retry rule; opening re-attributes, `T-022-08`); residual R-REOPEN |
| `components/core-audit/src/log/signatures.rs` | 30 | 0 | sound |
| `components/core-audit/src/log/store.rs` | 143 | 0 | sound; residual R-FSYNC |
| `components/core-audit/src/measures/compute.rs` | 421 | 0 | sound |
| `components/core-audit/src/measures/mod.rs` | 38 | 0 | sound |
| `components/core-audit/src/measures/record.rs` | 92 | 0 | sound |
| `components/core-audit/src/measures/trail.rs` | 110 | 0 | sound |
| `components/core-audit/src/receipts/mod.rs` | 43 | 0 | sound |
| `components/core-audit/src/receipts/receipt.rs` | 328 | 0 | sound (log key, shape-separated, strict verifier) |
| `components/core-audit/src/receipts/store.rs` | 831 | 0 | sound (policy asked before any lookup) |
| `components/core-audit/src/statements/disk.rs` | 147 | 0 | sound |
| `components/core-audit/src/statements/mod.rs` | 320 | 0 | sound |
| `components/core-audit/tests/checkpoints.rs` | 1592 | 0 | test; sound |
| `components/core-audit/tests/compare.rs` | 1786 | 0 | test; sound |
| `components/core-audit/tests/log.rs` | 3658 | 0 | test; sound |
| `components/core-audit/tests/measures.rs` | 1168 | 0 | test; sound |
| `components/core-audit/tests/receipts.rs` | 1633 | 0 | test; sound |
| `components/core-audit/tests/reopen.rs` | 1174 | 0 | test; sound |
| `components/core-audit/tests/statements.rs` | 860 | 0 | test; sound |
| `components/core-authz/Cargo.toml` | 5 | 0 | sound (no new crate of ours; boundary holds) |
| `components/core-authz/src/cpl/mod.rs` | 353 | 0 | sound |
| `components/core-authz/src/cpl/tests.rs` | 943 | 0 | test; sound |
| `components/core-authz/src/engine/entry_points.rs` | 1171 | 0 | sound |
| `components/core-authz/src/engine/mod.rs` | 2784 | 0 | sound (one consultation; refusal beats not known; fails closed) |
| `components/core-authz/src/funds/mod.rs` | 644 | 0 | sound (`held` never permits; floor compared by value) |
| `components/core-authz/src/funds/tests.rs` | 1863 | 0 | test; sound |
| `components/core-authz/src/lib.rs` | 16 | 0 | sound |
| `components/core-authz/src/parties/mod.rs` | 572 | 0 | sound |
| `components/core-authz/src/parties/tests.rs` | 1612 | 0 | test; sound |
| `components/core-authz/src/roles/mod.rs` | 443 | 0 | sound |
| `components/core-authz/src/roles/tests.rs` | 1728 | 0 | test; sound |
| `components/core-authz/src/session/mod.rs` | 511 | 0 | sound |
| `components/core-authz/src/session/tests.rs` | 1284 | 0 | test; sound |
| `components/core-authz/src/stuck/mod.rs` | 697 | 0 | sound (separate permissions, operator role, holder and customer excluded) |
| `components/core-authz/src/stuck/tests.rs` | 1418 | 0 | test; sound |
| `components/prov/src/egress/mod.rs` | 4 | 0 | sound |
| `components/prov/src/egress/resolve.rs` | 29 | 6 | sound |
| `components/prov/src/egress/tls.rs` | 14 | 3 | sound |
| `components/prov/src/module/admission.rs` | 13 | 0 | sound (`safe_retry` read through the sealed judge) |
| `components/prov/src/module/bin/jail_probe.rs` | 224 | 0 | sound (`required-features = ["jail-layers"]`) |
| `components/prov/src/module/engine.rs` | 10 | 2 | sound (48.0.9 pin; `config_for` names the target) |
| `components/prov/src/module/jail.rs` | 37 | 0 | sound (feature-gated, `T-025-02`) |
| `components/prov/src/module/layers.rs` | 171 | 0 | sound (shipped `Layers` is a unit whose `on` is always true) |
| `components/prov/src/module/mod.rs` | 5 | 0 | sound |
| `components/prov/src/module/platform.rs` | 318 | 16 | sound for M3; medium M1 (repeat record in memory, lost on restart) |
| `components/prov/src/module/sandbox.rs` | 100 | 24 | sound (shipped paths take `Layers::ALL`) |
| `components/prov/src/module/types.rs` | 2 | 0 | sound (`StateEvent.transaction`) |
| `components/prov/src/proxy/https.rs` | 89 | 22 | sound (one deadline per exchange, tighter than before) |
| `components/prov/src/proxy/tests/deadline.rs` | 44 | 12 | test; sound |
| `conformance/evidence/v0-scope` | 287 | 0 | sound; classes match `T-024-09`; low L3 (a second `strict:` line wins unseen) |
| `contracts/CHANGELOG.md` | 33 | 0 | docs; sound |
| `contracts/CONTRACTS` | 1 | 0 | docs; sound |
| `contracts/README.md` | 11 | 3 | docs; sound |
| `contracts/core-internal/CHANGELOG.md` | 939 | 0 | docs; sound |
| `contracts/core-internal/README.md` | 888 | 0 | docs; sound |
| `contracts/core-internal/fixtures/admission/acceptance-is-asked-in-the-users-session--expires_at.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-is-asked-in-the-users-session--holder.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-is-asked-in-the-users-session--missing.json` | 873 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-is-of-the-quote-shown--missing.json` | 884 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-is-of-the-quote-shown--order.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-is-of-the-quote-shown--shown_as.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-rests-on-the-reserves-yes--another-parts-maker.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-rests-on-the-reserves-yes--maker.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-rests-on-the-reserves-yes--missing.json` | 874 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-rests-on-the-reserves-yes--order.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-rests-on-the-reserves-yes--transaction.json` | 918 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-rests-on-the-reserves-yes--verified.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/afterwards-rests-on-a-statement--about.json` | 2367 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/afterwards-rests-on-a-statement--missing.json` | 2323 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/answer-follows-its-question--an-arrival-answer-after-an-operation.json` | 5649 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/answer-follows-its-question--an-arrival-answer-after-its-question.json` | 5649 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/answer-follows-its-question--an-arrival-answer-with-no-question-before-it.json` | 5604 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/answer-follows-its-question--maker.json` | 132 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/answer-follows-its-question--missing.json` | 87 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/answer-follows-its-question--order.json` | 132 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/answer-follows-its-question--verified.json` | 132 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-earlier-arrival--deposit.json` | 5804 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-earlier-arrival--missing.json` | 5804 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-earlier-arrival--reserve.json` | 5855 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-notice--a-hold-cited-as-its-notice.json` | 2467 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-notice--an-arrival-answer-cited-as-its-notice.json` | 2467 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-notice--maker.json` | 2291 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-notice--missing.json` | 2291 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-notice--the-reserves-arrival-notice.json` | 2423 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-notice--transaction.json` | 2384 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-names-its-notice--verified.json` | 2433 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-rests-on-the-reserves-report--about.json` | 2290 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-rests-on-the-reserves-report--maker.json` | 2291 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-rests-on-the-reserves-report--missing.json` | 2246 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/arrival-rests-on-the-reserves-report--verified.json` | 2291 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/asked-before-is-complete--conflict.json` | 1384 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/asked-before-is-complete--refused.json` | 1385 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/automatic-deposit-rests-on-its-arrival--arrived.json` | 3731 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/automatic-deposit-rests-on-its-arrival--missing.json` | 3731 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/automatic-deposit-rests-on-its-arrival--reserve.json` | 3731 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/automatic-deposit-rests-on-its-arrival--second-acceptance-claiming-a-deposit.json` | 4834 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/changed-destination-rests-on-its-acceptance--missing.json` | 5252 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/changed-destination-rests-on-its-acceptance--order.json` | 5311 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-its-result--about.json` | 1998 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-its-result--its-providers-result.json` | 2055 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-its-result--maker.json` | 1998 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-its-result--missing.json` | 1998 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-its-result--provider.json` | 1998 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-its-result--result.json` | 1998 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-its-result--verified.json` | 2044 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-terms-in-force--in_force.json` | 2047 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-terms-in-force--maker.json` | 2047 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-terms-in-force--missing.json` | 2006 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-terms-in-force--on-the-price-its-route-was-priced-on.json` | 1998 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-terms-in-force--terms.json` | 2006 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/charge-raise-rests-on-terms-in-force--verified.json` | 2047 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/chosen-route-rests-on-its-orders-acceptance--missing.json` | 1137 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/chosen-route-rests-on-its-orders-acceptance--origin_reserve.json` | 1220 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/chosen-route-rests-on-its-orders-acceptance--parts-out-of-order.json` | 1220 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/chosen-route-rests-on-its-orders-acceptance--parts.json` | 1208 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/customer-read-is-of-its-own-order--missing.json` | 2806 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/customer-read-is-of-its-own-order--order-state-of-an-order-not-its-own.json` | 2806 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/customer-read-is-of-its-own-order--order-state-of-another-order.json` | 2806 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/customer-read-is-of-its-own-order--order-state-of-its-order.json` | 2806 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/customer-read-is-of-its-own-order--party.json` | 2806 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/customer-read-is-of-its-own-order--segment-of-its-order.json` | 2808 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/customer-read-is-of-its-own-order--segment.json` | 2808 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-is-for-its-caller--its-caller.json` | 1392 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-is-for-its-caller--missing.json` | 1392 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-record-chains-to-its-orders-previous--missing.json` | 796 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-record-chains-to-its-orders-previous--order.json` | 836 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--auth1.json` | 975 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--charge.json` | 5503 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--decide.json` | 1393 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--decide2.json` | 1695 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--decide_erase.json` | 3037 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--grant.json` | 2624 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--grant2.json` | 2718 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--post.json` | 2574 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--q1.json` | 1277 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--q2.json` | 1331 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--read.json` | 2667 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--read2.json` | 2764 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--read_c.json` | 2807 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--read_own.json` | 2898 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--read_p.json` | 2852 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--report_es.json` | 3977 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--revoke.json` | 2948 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--sd.json` | 3541 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--sdh.json` | 4139 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--session.json` | 42 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--settled_es.json` | 4043 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--sm.json` | 3586 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/decision-through-the-general-handle--smh.json` | 4184 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/delivery-goes-to-the-parts-ends--another-account.json` | 3540 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/delivery-goes-to-the-parts-ends--another-origin-account.json` | 3540 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/delivery-goes-to-the-parts-ends--another-origin-node.json` | 3540 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/delivery-goes-to-the-parts-ends--missing.json` | 3485 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/delivery-goes-to-the-parts-ends--moved.json` | 3652 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/delivery-goes-to-the-parts-ends--origin.json` | 3540 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/delivery-goes-to-the-parts-ends--to.json` | 3540 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-has-one-result--after-the-settled-one.json` | 4116 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-has-one-result--after-the-settlement.json` | 3974 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-has-one-result--conflict.json` | 1530 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-has-one-result--retried.json` | 1530 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-permits-a-decided-action--action.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-permits-a-decided-action--decision.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-permits-a-decided-action--erase-without-a-decision.json` | 3038 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-permits-a-decided-action--missing.json` | 1394 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/effect-permits-a-decided-action--request.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/ended-part-takes-no-more--conflict.json` | 4455 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-is-carried-out-on-its-decision--done.json` | 3156 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-is-carried-out-on-its-decision--missing.json` | 3119 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-is-carried-out-on-its-decision--outcome.json` | 3156 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-names-a-kept-statement--missing.json` | 3115 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-waits-while-anything-rests-on-the-statement--conflict.json` | 3156 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-waits-while-anything-rests-on-the-statement--decided-while-cited.json` | 3036 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-waits-while-anything-rests-on-the-statement--effect.json` | 3141 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--about-grade-1.json` | 1535 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--about-grade-4.json` | 1532 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--about-segment.json` | 1978 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--about.json` | 1535 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--bound.json` | 1532 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--missing.json` | 1526 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--provider-word.json` | 1537 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--provider.json` | 1537 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--public-record-unverified-bound.json` | 1537 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--signed-and-verified.json` | 1537 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/evidence-rests-on-statements--verified.json` | 1532 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/fee-rests-on-a-settled-segment--missing.json` | 1749 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/fee-rests-on-a-settled-segment--order.json` | 1797 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/final-deposit-closes-on-a-public-record--conflict-naming-another-node.json` | 2088 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/final-deposit-closes-on-a-public-record--conflict-on-another-network.json` | 2087 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/final-deposit-closes-on-a-public-record--conflict-resting-on-another-statement.json` | 2087 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/final-deposit-closes-on-a-public-record--conflict-without-its-movement.json` | 2083 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/final-deposit-closes-on-a-public-record--conflict.json` | 2083 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/first-move-rests-on-its-parts-yes--automatic-deposit-by-anyone.json` | 1417 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/first-move-rests-on-its-parts-yes--automatic-deposit.json` | 1417 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/first-move-rests-on-its-parts-yes--missing.json` | 1444 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/first-move-rests-on-its-parts-yes--origin_reserve.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/first-move-rests-on-its-parts-yes--provider.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-input-rests-on-the-linked-orders-acceptance--a-waiting-part-on-the-customers-acceptance.json` | 5328 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-input-rests-on-the-linked-orders-acceptance--given.json` | 5313 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-input-rests-on-the-linked-orders-acceptance--missing.json` | 4270 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-input-rests-on-the-linked-orders-acceptance--order.json` | 4329 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-input-rests-on-the-linked-orders-acceptance--recorded-by-the-operator.json` | 5328 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-input-rests-on-the-linked-orders-acceptance--requester.json` | 4329 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-links-an-unopened-order--a-second-part-to-the-same-order.json` | 4377 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-links-an-unopened-order--after-a-hand-on-to-it-was-taken-back.json` | 4492 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-links-an-unopened-order--missing.json` | 4575 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-rests-on-the-linked-orders-acceptance--missing.json` | 4185 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-rests-on-the-linked-orders-acceptance--requester.json` | 4244 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-taken-back-rests-on-the-linked-orders-refusal--missing.json` | 4376 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-taken-back-rests-on-the-linked-orders-refusal--on-the-linked-orders-refusal.json` | 4408 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hand-on-taken-back-rests-on-the-linked-orders-refusal--unopened.json` | 4570 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-points-back--linked_order.json` | 4491 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-points-back--missing.json` | 4443 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--a-customers-hand-on-held-by-another-than-its-first-provider.json` | 4638 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--a-customers-hand-on-held-by-its-first-provider.json` | 4636 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--a-first-move-from-another-node.json` | 4758 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--a-holder-the-hand-on-did-not-name.json` | 4758 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--a-wallet-holds-and-the-plans-provider-runs-the-first-move.json` | 4756 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--holder.json` | 4748 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--missing.json` | 4617 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--node.json` | 4758 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/handed-on-route-starts-on-its-holding-record--provider.json` | 4758 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/held-part-takes-no-move--conflict.json` | 5553 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/held-part-takes-no-move--stuck.json` | 3560 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/held-quote-rests-on-the-reserves-hold--hold.json` | 3886 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/held-quote-rests-on-the-reserves-hold--maker.json` | 3930 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/held-quote-rests-on-the-reserves-hold--missing.json` | 3886 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/held-quote-rests-on-the-reserves-hold--the-reserves-hold.json` | 3930 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/held-quote-rests-on-the-reserves-hold--transaction.json` | 3927 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/held-quote-rests-on-the-reserves-hold--verified.json` | 3930 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hold-is-redeemed-by-one-order--conflict.json` | 3997 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hold-is-redeemed-by-one-order--the-same-acceptance-again.json` | 3997 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hold-is-redeemed-by-one-order--the-same-hold-kept-again-for-its-own-order.json` | 4041 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/hold-is-redeemed-by-one-order--the-same-hold-kept-again.json` | 4041 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-rests-on-its-record--labelled-its-publishers-word.json` | 4755 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-rests-on-its-record--maker.json` | 4622 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-rests-on-its-record--missing.json` | 4622 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-rests-on-its-record--order.json` | 4622 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-rests-on-its-record--publisher.json` | 4755 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-rests-on-its-record--result.json` | 4622 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-rests-on-its-record--transaction.json` | 4621 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-rests-on-its-record--verified.json` | 4622 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-starts-a-handed-on-route--missing.json` | 4622 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/holding-withdrawal-starts-a-handed-on-route--node.json` | 4754 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-cites-its-decision-record--missing.json` | 1180 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-cites-its-decision-record--no-route-on-a-shortlist-keeping-none.json` | 5542 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-cites-its-decision-record--no-route-on-another-orders-empty-shortlist.json` | 5542 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-cites-its-decision-record--order.json` | 1220 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--a-customers-hand-on-without-a-failed-final-deposit.json` | 5233 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--a-new-destination-without-a-failed-final-deposit.json` | 5216 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--a-refused-input-again-after-the-order-moved-on.json` | 5405 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--another-input-under-a-refused-inputs-record-id.json` | 4491 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--another-input-under-a-used-record-id.json` | 1680 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--another-input-under-an-earlier-record-id.json` | 1679 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--handed-on-while-not-held.json` | 3794 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--missing.json` | 4939 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--reversal-of-a-segment-never-started.json` | 2027 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-is-one-the-state-machine-takes--the-same-input-again.json` | 1680 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-not-taken-is-one-refused--another-input-under-a-refused-inputs-record-id.json` | 4431 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-not-taken-is-one-refused--another-input-under-it-as-the-host-refuses.json` | 4430 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-not-taken-is-one-refused--another-reason.json` | 4379 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-not-taken-is-one-refused--missing.json` | 1683 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-not-taken-is-one-refused--the-same-input-again-for-another-reason.json` | 4431 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-not-taken-is-one-refused--the-same-trace-after-the-order-moved-on.json` | 4431 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/input-not-taken-is-one-refused--the-same-trace-again.json` | 4431 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/linked-order-opens-on-its-hand-on--after-the-hand-on-was-taken-back.json` | 4519 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/linked-order-opens-on-its-hand-on--missing.json` | 4662 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/measure-cites-entries-of-the-log--missing.json` | 2417 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--a-replacement-for-another-part.json` | 1514 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--a-restatement-sets-nothing.json` | 1649 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--another-part-replaced.json` | 1514 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--destination.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--floor.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--in_force.json` | 1514 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--missing.json` | 1401 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--order.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--part.json` | 1459 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--to_asset.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-authorisation--under-a-restatement-after-the-replacement.json` | 1566 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-orders-acceptance--authorisation.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-orders-acceptance--missing.json` | 1373 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-rests-on-its-orders-acceptance--part.json` | 1746 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-segment-is-in-its-orders-plan--missing.json` | 1210 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-segment-is-in-its-orders-plan--order.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-segment-is-in-its-orders-plan--provider.json` | 1758 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/move-segment-is-in-its-orders-plan--unplanned-segment.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/movement-evidences-one-segment--conflict.json` | 2068 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/movement-evidences-one-segment--retried.json` | 2068 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/no-formula-marks-a-charge-in-another-asset--asset.json` | 2040 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/no-formula-marks-a-charge-in-another-asset--in-another-asset.json` | 2040 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/no-formula-marks-a-charge-in-another-asset--missing.json` | 2040 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/nothing-cites-an-erased-statement--a-failed-erasure.json` | 3195 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/nothing-cites-an-erased-statement--another-statement.json` | 3196 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/nothing-cites-an-erased-statement--between-effect-and-result.json` | 3141 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/nothing-cites-an-erased-statement--conflict.json` | 3195 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/nothing-cites-an-erased-statement--done-before-its-record.json` | 3178 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/notice-summary-names-its-raise--missing.json` | 5598 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-confirmation-per-effect--conflict.json` | 4166 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-confirmation-per-effect--the-same-confirmation-again.json` | 4108 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-order-per-deposit--conflict.json` | 2388 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-order-per-deposit--other-figures.json` | 2343 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-order-per-deposit--the-same-arrival-again.json` | 2343 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-order-per-deposit--the-same-notice-kept-again.json` | 2433 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-question-per-resolution-transaction--conflict.json` | 3873 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-question-per-resolution-transaction--the-same-question-again.json` | 3873 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-report-per-effect--a-new-report-after-a-decline.json` | 4231 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-report-per-effect--conflict.json` | 4034 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-report-per-effect--the-same-report-again.json` | 4034 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-start-per-holding-record--conflict.json` | 4708 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-start-per-holding-record--the-same-record-again.json` | 4663 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-summary-per-notice--another-receivers.json` | 5637 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-summary-per-notice--conflict.json` | 5637 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/one-summary-per-notice--the-same-summary-again.json` | 5637 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-a-declared-stuck-part-is-resolved--missing.json` | 3540 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-a-declared-stuck-part-is-resolved--no-plan-and-no-acceptance.json` | 3550 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-a-declared-stuck-part-is-resolved--no-plan-with-its-acceptance.json` | 3688 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-a-declared-stuck-part-is-resolved--stuck.json` | 3830 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-a-declared-stuck-part-is-resolved--unresolved.json` | 3748 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-a-declared-stuck-part-is-resolved--value-came-back.json` | 4061 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-a-declared-stuck-part-is-resolved--value-moved-on.json` | 4014 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-customer-sets-new-terms--conflict.json` | 1084 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-customer-sets-new-terms--operator-new-destination.json` | 1084 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-customer-sets-new-terms--restatement-after-the-replacement.json` | 1084 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-customer-sets-new-terms--restatement.json` | 1029 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-customer-sets-new-terms--the-requesters-party-in-another-role.json` | 1029 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-payer-moves-the-origin-account--a-restatement.json` | 1167 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-payer-moves-the-origin-account--missing.json` | 1167 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-payer-moves-the-origin-account--restated-after-the-payer-moved-it.json` | 1222 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-payer-moves-the-origin-account--the-operator.json` | 1167 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/only-the-payer-moves-the-origin-account--the-payer.json` | 1167 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/operator-command-is-the-deployment-operators--a-party-named-as-the-operators-maker.json` | 5462 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/operator-command-is-the-deployment-operators--a-provider.json` | 5458 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/operator-command-is-the-deployment-operators--missing.json` | 5458 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/order-holds-one-acceptance-basis--conflict.json` | 4834 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/order-is-opened-once--conflict.json` | 1466 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/order-is-opened-once--retried.json` | 1466 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-decline-on-cores-question.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-deliver-reported-on-an-unrelated-makers-statement.json` | 4108 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-on-reported-on-another-makers-statement.json` | 5847 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-on-through-a-question-after-its-result.json` | 5892 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-on-through-a-segment-of-its-linked-order.json` | 5894 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-on-through-a-statement-naming-no-order.json` | 5846 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-on-through-a-transaction-bound-by-no-operator-question.json` | 5892 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-on-through-a-transaction-bound-to-another-order.json` | 5892 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-on-through-another-effects-transaction.json` | 5945 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-on-through-another-order.json` | 5892 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-ons-confirmation-on-another-makers-statement.json` | 5958 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-hand-ons-report-on-the-holding-accounts-own-answer.json` | 5847 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-planless-part-through-an-arrival-of-an-order-of-two-parts.json` | 4160 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-planless-part-through-another-parts-arrival.json` | 4104 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-planless-part-through-another-transaction.json` | 4051 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-planless-part-through-its-acceptance.json` | 4006 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-planless-part-through-its-arrival.json` | 4103 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-planless-part-through-its-own-arrival.json` | 4104 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-planless-part-through-its-resolutions-transaction.json` | 4051 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-stuck-resolution-through-a-segment-of-its-part.json` | 4022 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--about-another-part.json` | 4042 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--about-another-segment-of-its-part.json` | 3880 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--about.json` | 4021 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--before-its-move.json` | 3976 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--cores-question-re-appended.json` | 4021 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--made-before-its-move.json` | 4021 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--missing.json` | 3976 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--the-operators-own-result.json` | 4021 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--the-providers-before-its-move.json` | 4021 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--verified.json` | 4021 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-move--missing.json` | 4042 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-move--unknown.json` | 3718 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-report--another-outcome.json` | 4042 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-report--declined.json` | 4239 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-report--evidence-by-the-same-maker.json` | 4087 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-report--evidence.json` | 4042 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-report--login.json` | 4042 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-report--missing.json` | 3984 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-report--on-one-actors-word.json` | 3900 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-rests-on-its-report--party.json` | 4042 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-decline-rests-on-its-report--a-second-decline-of-the-same-report.json` | 4238 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-decline-rests-on-its-report--evidence.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-decline-rests-on-its-report--login.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-decline-rests-on-its-report--missing.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-decline-rests-on-its-report--open.json` | 4239 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-decline-rests-on-its-report--party.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-decline-rests-on-its-report--the-same-decline-again.json` | 4238 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-report-rests-on-its-move--a-move-value-reported-by-another.json` | 3834 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-report-rests-on-its-move--a-move-value-reported-by-its-mover.json` | 3834 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-report-rests-on-its-move--after-the-final-result.json` | 4137 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-report-rests-on-its-move--missing.json` | 3976 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-report-rests-on-its-move--mover.json` | 3976 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-report-rests-on-its-move--open.json` | 4100 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-report-rests-on-its-move--unknown.json` | 3710 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/own-evidence-read-is-of-the-readers-statement--maker.json` | 2897 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/own-evidence-read-is-of-the-readers-statement--missing.json` | 2897 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/own-evidence-read-is-of-the-readers-statement--operator-login-of-another-party.json` | 2897 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/own-evidence-read-is-of-the-readers-statement--operator-login.json` | 2897 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/own-evidence-read-is-of-the-readers-statement--verified.json` | 2897 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/owner-change-rests-on-its-destination-check--missing.json` | 5381 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/owner-change-rests-on-its-destination-check--no-check.json` | 5343 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-evidence-rests-on-statements--a-part-with-no-resolution.json` | 3778 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-evidence-rests-on-statements--about.json` | 3778 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-evidence-rests-on-statements--another-part.json` | 3778 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-evidence-rests-on-statements--missing.json` | 3778 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-evidence-rests-on-statements--provider.json` | 3823 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-evidence-rests-on-statements--the-moving-providers-word.json` | 3778 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-evidence-rests-on-statements--verified.json` | 3823 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--a-resolution-moved-while-another-moves.json` | 3911 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--a-second-resolution-while-one-moves.json` | 3711 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--a-silent-move-settled-on-two-actors.json` | 3965 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--a-silent-move.json` | 3841 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--a-start-undone-with-a-move-in-motion.json` | 5113 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--a-start-undone-with-nothing-moving.json` | 4987 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--an-end-requested-while-a-part-past-its-start-moves.json` | 5020 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--an-end-requested-with-nothing-moving.json` | 5266 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--an-end-requested-with-p2s-start-in-motion.json` | 5020 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--another-part.json` | 4613 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--changed-destination.json` | 5112 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--conflict.json` | 4707 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--done.json` | 4744 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--failed-deposit.json` | 2408 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--failed-exchange.json` | 4744 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--failed.json` | 4650 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--floor-wait.json` | 5553 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--handed-on-with-a-move-in-motion.json` | 5082 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-waits-for-value-in-motion--recalculated-around-a-resolution.json` | 3785 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permit-backs-one-effect--conflict.json` | 1520 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permit-backs-one-effect--retried.json` | 1520 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permitted-read-is-not-revoked--conflict.json` | 2716 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permitted-read-rests-on-its-grant--grantee.json` | 2666 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permitted-read-rests-on-its-grant--missing.json` | 2616 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permitted-read-rests-on-its-grant--order-state-under-another-order.json` | 2668 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permitted-read-rests-on-its-grant--order-state-under-its-order.json` | 2668 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permitted-read-rests-on-its-grant--order-state-under-the-trail.json` | 2667 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permitted-read-rests-on-its-grant--scope-of-another-order.json` | 2763 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/permitted-read-rests-on-its-grant--scope.json` | 2667 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/posted-route-cites-its-decision-record--missing.json` | 2529 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/posted-route-cites-its-decision-record--order.json` | 2573 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/provider-asked-before-was-asked--missing.json` | 1274 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/provider-asked-before-was-asked--order.json` | 1330 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/provider-asked-before-was-asked--refused.json` | 1275 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/provider-read-is-of-its-own-segment--missing.json` | 2787 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/provider-read-is-of-its-own-segment--provider.json` | 2851 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/public-mismatch-rests-on-the-providers-result--about.json` | 5616 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/public-mismatch-rests-on-the-providers-result--maker.json` | 5616 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/public-mismatch-rests-on-the-providers-result--missing.json` | 5616 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/public-mismatch-rests-on-the-providers-result--non_final.json` | 2096 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/public-mismatch-rests-on-the-providers-result--result.json` | 5616 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/public-mismatch-rests-on-the-providers-result--verified.json` | 5662 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/raise-cites-entries-of-the-log--missing.json` | 2479 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/recalculation-starts-from-evidenced-value--an-earlier-settlement.json` | 1860 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/recalculation-starts-from-evidenced-value--missing.json` | 1717 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/recalculation-starts-from-evidenced-value--node.json` | 4755 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/recalculation-starts-from-evidenced-value--part.json` | 4565 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-about-the-next-segment--a-segment-of-another-part.json` | 2417 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-about-the-next-segment--missing.json` | 2417 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-about-the-next-segment--next.json` | 2417 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-about-the-next-segment--the-first-segment-of-a-recalculated-route.json` | 2596 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-about-the-next-segment--the-segment-before-it.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-its-partys-statement--a-statement-about-nothing.json` | 2366 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-its-partys-statement--about-the-sent-segment.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-its-partys-statement--about.json` | 2369 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-its-partys-statement--maker.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-its-partys-statement--missing.json` | 2325 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-its-partys-statement--one-publishers-two-providers.json` | 2370 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-its-partys-statement--result.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/received-report-is-its-partys-statement--verified.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/record-through-policys-handle--effect.json` | 1457 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/record-through-policys-handle--result.json` | 1494 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/record-through-policys-handle--s_bridge.json` | 530 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/refiling-files-an-arrival-anew--a-party-named-as-the-operators-maker.json` | 2333 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/refiling-files-an-arrival-anew--a-stated-operator.json` | 2329 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/refiling-files-an-arrival-anew--another-operator.json` | 2329 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/refiling-files-an-arrival-anew--filer.json` | 2329 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/refiling-files-an-arrival-anew--missing.json` | 2277 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/refiling-files-an-arrival-anew--order.json` | 2329 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/refiling-files-an-arrival-anew--the-operator.json` | 2329 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/refiling-files-an-arrival-anew--the-operators-party-not-its-maker.json` | 2333 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reliance-names-its-judges-statement--about.json` | 2411 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reliance-names-its-judges-statement--maker.json` | 2408 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reliance-names-its-judges-statement--missing.json` | 2367 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reliance-names-its-judges-statement--verified.json` | 2408 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--a-node-that-recorded-nothing.json` | 1862 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--a-sibling-of-the-exits-provider.json` | 1856 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--about.json` | 1858 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--another-provider-of-another-publisher.json` | 1858 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--its-nodes-record.json` | 1856 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--missing.json` | 1813 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--node.json` | 1858 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--once.json` | 1912 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--one-record-of-each-of-two-nodes.json` | 1910 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--publisher.json` | 1858 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--record.json` | 1858 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-cites-its-nodes-records--verified.json` | 1858 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-rests-on-its-exit--missing.json` | 1805 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-rests-on-its-exit--order.json` | 1805 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/replay-rests-on-its-exit--segment.json` | 1805 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/report-pair-is-by-its-segments-providers--a-received-report-by-another-than-its-segments-provider.json` | 2417 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/report-pair-is-by-its-segments-providers--missing.json` | 2307 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/report-pair-is-by-its-segments-providers--provider.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/result-ends-an-effect--missing.json` | 1429 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/retired-segment-takes-no-move--changed-destination.json` | 5437 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/retired-segment-takes-no-move--conflict.json` | 4881 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/retired-segment-takes-no-move--double-deposit.json` | 4739 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/retired-segment-takes-no-move--settled-segment.json` | 4881 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-is-of-a-reversal-owed--after-the-reversal-completed.json` | 2153 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-is-of-a-reversal-owed--missing.json` | 2105 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--a-label-alone.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--a-refused-decision.json` | 2227 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--a-session-never-opened.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--action.json` | 2219 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--an-accept-fault-before-the-showing.json` | 2205 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--an-impostors-command.json` | 2205 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--appended-before-the-showing.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--decided-before-the-showing-was-received.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--missing.json` | 2173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--naming-another-showing.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--naming-another-showings-commitment.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--party.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--session.json` | 2272 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--showing.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--the-customers-decision-in-its-session.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--the-operators-command.json` | 2205 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-acceptance--yes.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-showing--about.json` | 2173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-showing--its-provider.json` | 2105 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-showing--maker.json` | 2173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-showing--missing.json` | 2127 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-showing--result.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-showing--the-customer-on-the-providers-showing.json` | 2226 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-rests-on-its-showing--verified.json` | 2173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-stands-once-the-party-at-fault-carries-it--conflict.json` | 2312 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-stands-once-the-party-at-fault-carries-it--the-provider-after-the-customer.json` | 2258 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-stands-once-the-party-at-fault-carries-it--the-provider-then-the-customer.json` | 2258 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-carrier-stands-once-the-party-at-fault-carries-it--the-same-carrier-again.json` | 2280 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-is-asked-of-its-failed-provider--missing.json` | 2167 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-is-asked-of-its-failed-provider--of-a-settled-segment.json` | 2167 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-is-asked-of-its-failed-provider--of-the-failed-provider.json` | 2167 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--a-publishers-word-labelled.json` | 2169 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--a-publishers-word-on-a-node-its-record-binds.json` | 2214 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--a-publishers-word-on-another-nodes-record.json` | 2216 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--a-replay-on-another-orders-statement.json` | 2214 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--at-a-later-inner-node-its-reserves-own-word.json` | 615 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--at-a-later-inner-node-the-failed-providers-own-word.json` | 615 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--at-a-later-inner-node-the-nodes-reserve-on-anothers-failure.json` | 615 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--maker.json` | 2121 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--missing.json` | 2077 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--order.json` | 2121 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--private.json` | 2122 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--publisher.json` | 2215 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--reserve.json` | 2121 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--segment.json` | 2121 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--the-origin-reserves-own-word-at-a-later-inner-node.json` | 615 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/reversal-rests-on-its-evidence--whose.json` | 2121 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/revocation-revokes-a-grant--missing.json` | 2897 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/revocation-revokes-a-grant--refused.json` | 2898 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/route-starts-on-the-withdrawal-proof--missing.json` | 1710 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/segment-belongs-to-one-order--conflict.json` | 1520 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/segment-is-moved-once--after-a-failed-move.json` | 1619 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/segment-is-moved-once--conflict.json` | 1619 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/sent-report-is-its-partys-statement--a-statement-about-nothing.json` | 2366 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/sent-report-is-its-partys-statement--about-the-received-segment.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/sent-report-is-its-partys-statement--about.json` | 2369 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/sent-report-is-its-partys-statement--maker.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/sent-report-is-its-partys-statement--missing.json` | 2325 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/sent-report-is-its-partys-statement--one-publishers-two-providers.json` | 2370 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/sent-report-is-its-partys-statement--result.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/sent-report-is-its-partys-statement--verified.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/settlement-rests-on-its-evidence--missing.json` | 1588 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/settlement-rests-on-its-evidence--order.json` | 1632 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/settlement-rests-on-its-evidence--segment.json` | 1632 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/shortlisted-authority-cites-its-shortlist--asking-a-verifier-it-needs.json` | 1283 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/shortlisted-authority-cites-its-shortlist--missing.json` | 1236 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/shortlisted-authority-cites-its-shortlist--order.json` | 1276 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/shortlisted-authority-cites-its-shortlist--providers.json` | 1275 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/shortlisted-authority-cites-its-shortlist--refused.json` | 1237 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/shortlisted-authority-cites-its-shortlist--round.json` | 1272 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/shortlisted-authority-cites-its-shortlist--verifiers.json` | 1279 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/split-deposit-arrival-names-its-part--each-part.json` | 4556 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/split-deposit-arrival-names-its-part--missing.json` | 4554 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-of-the-segments-provider--missing.json` | 5655 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--a-copy-of-its-own-decision-at-another-receipt.json` | 5798 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--a-decision-copying-its-own-ids-for-another-action.json` | 5798 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--a-stuck-resolutions-transaction.json` | 5751 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--a-transaction-a-hand-over-names.json` | 5797 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--a-transaction-an-accepted-showing-names.json` | 5804 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--a-transaction-another-late-status-decision-names.json` | 5702 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--a-transaction-only-a-resolve-stuck-effect-names.json` | 5692 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--a-transaction-only-a-stuck-decide-step-names.json` | 5690 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--another-late-status-under-its-own.json` | 5749 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--conflict.json` | 5655 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/status-is-asked-under-a-fresh-transaction--the-transaction-of-another-late-status.json` | 5751 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-decision-is-of-a-stuck-order--missing.json` | 3540 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-decision-is-of-a-stuck-order--observed.json` | 3493 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-moves-the-decided-resolution--missing.json` | 3587 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-moves-the-decided-resolution--order.json` | 3652 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-moves-the-decided-resolution--resolution.json` | 3652 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-moves-the-decided-resolution--transaction.json` | 4244 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-permits-a-stuck-move--carries_out.json` | 3652 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-permits-a-stuck-move--decision.json` | 3652 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-permits-a-stuck-move--missing.json` | 3669 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-permits-a-stuck-move--order.json` | 3652 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-effect-permits-a-stuck-move--request.json` | 3652 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-move-carries-out-its-decide-step--login.json` | 3585 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-move-carries-out-its-decide-step--missing.json` | 3520 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-move-carries-out-its-decide-step--mover.json` | 3585 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-move-carries-out-its-decide-step--order.json` | 3585 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolution-transaction-is-its-own--a-decide-step-under-the-deliverys.json` | 4138 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolution-transaction-is-its-own--another-questions-transaction.json` | 4228 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolution-transaction-is-its-own--its-own-question-and-answer.json` | 4334 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolution-transaction-is-its-own--p1-s1s-move.json` | 4244 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolution-transaction-is-its-own--p3s-yes.json` | 4244 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolution-transaction-is-its-own--the-deliverys.json` | 4244 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolution-transaction-is-its-own--the-deposits-notice.json` | 4244 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolution-transaction-is-unbound-or-its-orders--missing.json` | 4289 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-cites-its-parts-evidence--a-segments-evidence.json` | 3826 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-cites-its-parts-evidence--its-parts-evidence.json` | 3826 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-cites-its-parts-evidence--missing.json` | 3826 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-cites-its-parts-evidence--order.json` | 3826 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-cites-its-parts-evidence--part.json` | 3826 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-follows-a-done-resolution--another-part.json` | 3737 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-follows-a-done-resolution--delivered.json` | 3781 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-follows-a-done-resolution--done.json` | 3826 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-follows-a-done-resolution--handed-on-before-its-move-is-done.json` | 4292 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-follows-a-done-resolution--missing.json` | 3700 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/stuck-resolved-follows-a-done-resolution--to.json` | 3737 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/transaction-belongs-to-one-order--conflict.json` | 447 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/transaction-belongs-to-one-order--retried.json` | 177 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/transaction-belongs-to-one-order--segment.json` | 1889 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/unknown-outcome-settles-on-its-confirmation--missing.json` | 4013 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/unknown-outcome-settles-on-its-confirmation--outcome.json` | 4079 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/waiting-at-floor-rests-on-the-customers-floor--floor.json` | 5426 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/waiting-at-floor-rests-on-the-customers-floor--missing.json` | 5372 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/waiting-at-floor-rests-on-the-customers-floor--order.json` | 5427 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--acc1.json` | 919 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--acc2.json` | 3878 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--acc3.json` | 4981 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--accept_fault12.json` | 2151 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--after.json` | 2500 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--arrival.json` | 2423 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--arrival_earlier.json` | 5804 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--auth1.json` | 974 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--carrier12.json` | 2205 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--command.json` | 5590 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--decide.json` | 1392 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--decide2.json` | 1694 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--decide_erase.json` | 3168 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--decide_status.json` | 5606 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--decline.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--dest_owner.json` | 5481 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--dr1.json` | 792 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--dr10.json` | 4798 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--dr4.json` | 836 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--dr41.json` | 4548 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--effect.json` | 1456 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--effect2.json` | 1758 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--effect_erase.json` | 3218 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--effect_status.json` | 5655 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--eh.json` | 4376 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--erasure.json` | 3288 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--es.json` | 3784 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--ev16.json` | 5117 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--ev31.json` | 3424 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--ev7.json` | 1537 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--ev8.json` | 1979 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--evidence_movement.json` | 2020 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--evidence_movement_of_a_node.json` | 2069 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--fee.json` | 1797 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--final_deposit.json` | 2087 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--final_deposit_into_a_private_node.json` | 2088 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--grant.json` | 2755 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--grant2.json` | 2849 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--hold2.json` | 4754 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_bound11.json` | 1584 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_bound12.json` | 2026 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_bound16.json` | 5164 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_bound17.json` | 5259 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_bound31.json` | 3471 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_dest.json` | 5443 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_failed12.json` | 2073 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_failed17.json` | 5306 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_floor.json` | 5559 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_handed.json` | 4461 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_recalc.json` | 4887 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_reversal.json` | 2253 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_route.json` | 1220 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_route2.json` | 4623 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_settled.json` | 1632 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_settled16.json` | 5212 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_settled31.json` | 3519 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_stuck.json` | 3566 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--in_wait.json` | 5353 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--measure.json` | 2595 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--not_taken.json` | 4512 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--notice_summary.json` | 5598 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--pair.json` | 2371 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--part_evidence.json` | 3778 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--post.json` | 2705 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--public_mismatch.json` | 5616 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q1.json` | 1276 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q2.json` | 1330 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_bank.json` | 666 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_bridge.json` | 483 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_eubank1.json` | 177 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_fx.json` | 575 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_fx2.json` | 1843 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_hold2.json` | 4668 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_out3.json` | 3960 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_us3.json` | 3334 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_usbank1.json` | 87 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_usbank1c.json` | 267 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_usbank2.json` | 357 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--q_x16.json` | 5027 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--raise.json` | 2657 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--read.json` | 2798 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--read2.json` | 2895 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--read_c.json` | 2938 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--read_own.json` | 3029 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--read_p.json` | 2983 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--refiling.json` | 2461 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--refusal_owner.json` | 5513 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--reliance.json` | 2540 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--replay.json` | 1805 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--report_es.json` | 4108 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--result.json` | 1493 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--result_eh.json` | 4413 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--result_erase.json` | 3255 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--result_es.json` | 3915 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--result_es2.json` | 4211 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--revoke.json` | 3079 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_bank.json` | 711 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_bridge.json` | 529 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_eubank1.json` | 222 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_fx.json` | 621 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_fx2.json` | 1889 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_hold2.json` | 4713 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_kyc.json` | 752 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_out3.json` | 4005 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_out3c.json` | 4050 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_relied.json` | 3607 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_spare.json` | 3120 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_us3.json` | 3380 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_usb12.json` | 1935 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_usbank1.json` | 132 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_usbank1c.json` | 312 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_usbank2.json` | 402 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--s_x16.json` | 5073 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--sd.json` | 3672 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--sdh.json` | 4270 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--session.json` | 41 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--settled_es.json` | 4174 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--showing12.json` | 2119 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--shown1.json` | 437 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--shown2.json` | 3819 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--shown3.json` | 4922 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--sm.json` | 3717 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/world--smh.json` | 4315 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/refused--a-module-the-operator-publishes-in-the-operators-role.json` | 152 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/refused--a-role-its-maker-does-not-hold.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/refused--another-statement-kept.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/refused--attributed-by-its-writer.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/refused--no-statement-kept.json` | 52 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-key-that-does-not-sign-for-its-maker.json` | 53 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-party-its-maker-does-not-state-for.json` | 65 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-providers-operation-claiming-the-operator.json` | 64 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-release-whose-acceptance-failed.json` | 152 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-release-whose-acceptance-has-no-result.json` | 115 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-result-about-nothing-whose-body-names-a-transaction.json` | 54 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-revoked-releases-key-for-a-segment-on-another-version.json` | 276 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-segment-without-its-transaction.json` | 57 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-signature-that-does-not-verify.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-transaction-its-bytes-do-not-name.json` | 57 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-yes-replayed-for-another-order-with-its-transaction-left-out.json` | 57 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-actor-other-than-the-envelopes-maker.json` | 69 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-exploited-releases-key-even-in-flight.json` | 361 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-exploited-releases-key.json` | 237 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-about-its-node-alone-naming-another.json` | 242 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-about-the-order-naming-another-node.json` | 243 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-about-the-order-naming-no-node.json` | 242 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-naming-a-transaction-core-never-asked-under.json` | 243 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-naming-a-transaction-first-bound-to-another-order.json` | 334 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-naming-a-transaction-of-another-order.json` | 243 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-naming-a-transaction-the-log-binds-to-no-order.json` | 243 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-naming-another-publisher-than-its-maker-states-for.json` | 243 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-of-no-segments-about-the-order.json` | 243 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-inner-segments-record-whose-transactions-another-than-the-operator-asked-under.json` | 243 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-operator-module-key-no-accepted-release-names.json` | 158 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-operator-module-whose-release-was-revoked.json` | 237 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--an-order-without-a-signed-transaction.json` | 57 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--another-deployment.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--another-key.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--another-made-at.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--another-maker.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--another-shape.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--another-transaction.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--bytes-that-are-no-envelope.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-body-without-a-transaction-about-nothing.json` | 54 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-provider-stating-for-its-publisher.json` | 65 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-question-the-operator-signed.json` | 65 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-reinstated-releases-key.json` | 322 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-revoked-releases-key-for-a-segment-in-flight-on-it.json` | 276 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-revoked-releases-key-for-a-segment-that-moved-onto-it.json` | 283 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--an-inner-segments-record-about-its-node-alone.json` | 242 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--an-inner-segments-record-about-nothing.json` | 239 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--an-inner-segments-record-about-the-order-its-transactions-were-asked-under.json` | 243 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--an-operator-module-signed-with-its-releases-statement-key.json` | 152 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--the-bytes-bear-the-claim-out.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/authorisation-naming-a-part-twice.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/authorisation-naming-no-parts.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/authorisation-naming-two-orders.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/authorisation-with-a-floor-of-zero.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/authorisation-without-its-origin-reserve.json` | 23 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/authorisation-without-its-to-asset.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/charge-of-no-work.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/charge-to-another-party-than-its-callers.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/charge-to-no-party.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/checkpoint-carrying-its-salt.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/checkpoint-carrying-its-size.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/checkpoint-opening-without-its-salt.json` | 6 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/checkpoint-whose-tick-tells-its-size.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-permitting-without-a-receipt.json` | 7 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-refusing-without-its-class.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-by-an-actor-of-no-kind.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-marking-stale-without-a-version.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-moving-value-under-no-authorisation.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-moving-value-without-the-origin-reserve.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-moving-value-without-the-segments-provider.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-of-an-unknown-action.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-setting-slippage-on-a-version.json` | 17 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-setting-slippage-without-a-tuple.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-to-accept-a-release-without-its-statement-key.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-to-ask-a-late-status-without-its-segment.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-to-ask-a-late-status-without-its-transaction.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-to-erase-without-its-reason.json` | 17 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-to-grant-without-its-advertising-keys.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-to-mark-a-release-stale-naming-advertising-keys.json` | 19 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-to-move-a-stuck-orders-value.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/effect-citing-a-permit-without-its-receipt.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/effect-citing-a-refusal.json` | 45 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/effect-granting-without-a-tuple.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/effect-without-a-decision.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/effect-without-a-log-receipt.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/entry-of-no-kind.json` | 53 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-cross-module-call-without-its-transaction.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-limit-without-its-transaction.json` | 6 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-operation-ended-with-no-answer-without-its-transaction.json` | 5 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-operation-ended-with-no-answer-without-what-ended-it.json` | 5 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-outbound-call-without-its-transaction.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-secret-lookup-without-its-transaction.json` | 12 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-secret-use-carrying-the-value.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-secret-use-of-the-statement-key-naming-an-operation.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-secret-use-without-its-transaction.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-segment-record-running-on-a-version-it-never-moved-to.json` | 8 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-segment-record-whose-moves-skip-a-version.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-segment-record-without-its-transaction.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-segment-record-without-the-version-it-began-on.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/handover-state-read-without-its-transaction.json` | 7 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/holding-reading-naming-an-order-twice.json` | 22 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/holding-reading-without-its-order-attribution.json` | 12 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/kept-statement-with-a-short-salt.json` | 8 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/kept-statement-with-padded-bytes.json` | 8 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/measure-record-citing-nothing.json` | 17 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/measure-record-of-a-customer.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/measure-record-without-its-provider.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/measure-record-without-its-role.json` | 19 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/permission-change-by-a-provider.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/permission-change-granted-to-a-customer.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/permission-change-without-its-grantee.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/person-question-asking-a-provider-twice.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/person-question-asking-a-verifier-its-route-does-not-need.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/person-question-asking-no-one.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/person-question-asking-off-its-shortlist.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/person-question-on-a-shortlist-naming-no-providers.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/person-question-without-its-authority.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/person-question-without-the-providers-already-asked.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-carrying-an-order-state-change.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-citing-nothing.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-a-charge-marked-with-no-formula-naming-a-maximum.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-a-charge-within-its-maximum.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-a-charge-without-its-result.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-a-holding-not-comparable-without-its-asset.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-a-holding-not-comparable-without-its-word.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-a-public-end-whose-figures-agree.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-an-unread-account-naming-a-word.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-an-unread-account-naming-an-asset.json` | 23 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-reports-not-comparable-in-one-asset.json` | 41 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/raise-of-reports-that-agree.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/read-answer-without-a-receipt.json` | 7 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/read-check-of-a-segment-without-its-provider.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/read-check-of-nothing.json` | 12 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/read-check-without-the-readers-role.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/receipt-at-a-negative-index.json` | 8 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-filled-in-increments-of-nothing.json` | 44 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-of-one-party-reporting-both.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-reported-gross-without-its-charge.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-whose-charge-is-above-what-was-sent.json` | 44 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-whose-received-report-names-no-segment.json` | 42 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-whose-segment-is-not-its-sent-reports.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-whose-two-reports-are-about-one-segment.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-without-its-convention.json` | 41 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-without-its-terms.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-without-the-receiving-partys-report.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/report-pair-without-the-sending-partys-report.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/session-request-ending-before-it-is-asked.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-about-nothing.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-carrying-its-signatures-value.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-carrying-its-statement.json` | 76 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-naming-a-node-for-a-result.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-of-a-nodes-record-naming-a-transaction.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-relayed-by-its-own-maker.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-timed-by-cores-clock.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-unverified-but-attributed.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-without-its-commitment.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-without-the-key-it-was-signed-with.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stated-entry-without-the-source-of-its-time.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stuck-decide-naming-no-evidence.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/stuck-move-without-the-decision-it-carries-out.json` | 11 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-carrying-a-statement.json` | 92 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-charge-counted-without-when.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-charge-over-its-bound-said-counted.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-choice-record-naming-verifiers.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-command-that-is-a-catalogue-change.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-command-under-a-synonym.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-customers-permitted-read-of-the-trail.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-decision-observed.json` | 52 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-decision-record-without-its-digest.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-decision-record-without-its-id.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-decision-without-its-id.json` | 51 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-decline-naming-no-report.json` | 47 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-fee-to-a-third-party.json` | 29 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-hand-over-without-its-place-in-its-drain.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-holding-check-carrying-a-balance.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-holding-withdrawal-without-its-node.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-notice-summary-of-no-try.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-notice-summary-without-its-outcome.json` | 29 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-providers-permitted-read-of-an-order.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-providers-permitted-read-of-an-orders-state.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-public-mismatch-at-another-network.json` | 47 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-public-mismatch-whose-figures-agree.json` | 47 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-public-mismatch-without-its-segment.json` | 46 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-raise-merely-observed.json` | 52 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-refusal-carrying-a-providers-text.json` | 22 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-regulators-read-without-its-permission.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-citing-a-record-of-a-node-that-recorded-nothing.json` | 48 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-mismatch-against-a-node-not-replayed.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-mismatch-naming-no-way.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-naming-more-records-than-nodes.json` | 66 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-of-a-silent-node-not-replayed.json` | 40 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-of-no-nodes.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-recorded-as-proof.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-resting-on-no-receipt.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-replay-resting-on-nothing.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-session-outliving-its-until.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-settlement-without-its-declared-time.json` | 23 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-shortlist-record-without-its-providers.json` | 26 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-stuck-parts-evidence-resting-on-nothing.json` | 25 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-stuck-parts-evidence-without-its-part.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-acceptance-citing-one-yes-for-two-parts.json` | 61 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-acceptance-given-without-the-reserves-yes.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-acceptance-naming-a-part-twice.json` | 61 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-acceptance-without-its-id.json` | 48 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-acceptance-without-its-parties.json` | 44 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-effect-accepting-a-fault.json` | 45 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-effect-already-done.json` | 54 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-effect-without-a-permit.json` | 41 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-opened-session-without-its-session.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-graded-evidence-resting-on-nothing.json` | 25 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-graded-evidence-without-its-id.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-reliance-naming-no-judge.json` | 29 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-the-operators-permitted-read-of-an-order-without-a-permission.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-timed-by-a-partys-statement.json` | 52 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-without-the-source-of-its-time.json` | 51 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/authorisation-of-one-order-given-by-an-agent.json` | 23 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/authorisation-of-one-order-with-a-floor.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/authorisation-of-one-part-of-a-split-order.json` | 26 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/charge-answer-not-known.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/charge-answer-over.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/charge-answer-refused.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/charge-answer-within.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/charge-of-a-callers-own-request.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/charge-of-fanned-out-work.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/checkpoint-of-a-tick.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/checkpoint-opening-of-a-tick.json` | 7 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-not-known-as-policy-is-unreachable.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-permitting.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-refusing-outside-the-authorisation.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-accept-a-fault-in-the-customers-session.json` | 23 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-accept-a-module-release-with-its-statement-key.json` | 17 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-ask-a-providers-late-status.json` | 17 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-erase-a-kept-statement.json` | 18 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-grant-a-tuple-with-its-advertising-keys.json` | 19 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-mark-a-release-stale.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-move-value.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-post-a-suggested-route.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-promote-a-staged-release.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-set-a-tuples-slippage.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/effect-moving-value-on-a-permit.json` | 45 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/effect-resolving-a-stuck-order.json` | 48 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/entry-stated.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/entry-verified.json` | 54 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-cross-module-call.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-limit.json` | 7 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-operation-ended-with-no-answer.json` | 6 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-outbound-call.json` | 25 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-secret-lookup-of-a-sandboxed-part.json` | 10 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-secret-lookup-resolved.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-secret-use-of-a-secret.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-secret-use-of-the-statement-key-refused.json` | 12 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-segment-record-moved-once.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-segment-record-never-moved.json` | 8 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/handover-state-read.json` | 8 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/holding-reading-at-a-private-node.json` | 22 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/holding-reading-with-nothing-held.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/kept-statement-of-a-quote.json` | 8 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/measure-record-of-a-dry-run.json` | 22 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/measure-record-of-a-failure-against-the-market.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/measure-record-of-a-failure-with-no-market-figure.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/measure-record-of-a-quote-by-a-party-that-is-also-a-verifier.json` | 26 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/measure-record-of-a-quote.json` | 26 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/measure-record-of-a-refusal.json` | 19 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/measure-record-of-a-reversal.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/measure-record-of-an-answer.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/permission-change-granting-a-regulator-the-trail.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/person-answer-allowed.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/person-answer-past-the-bound.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/person-question-on-a-shortlisted-route-after-a-recalculation.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/person-question-on-a-shortlisted-route-asking-a-verifier-it-needs.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/person-question-on-the-persons-authority-first-asked.json` | 23 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-a-change-in-a-providers-p99.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-a-charge-above-its-maximum.json` | 42 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-a-charge-in-an-asset-no-formula-covers.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-a-holding-account-evidenced-in-another-asset.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-a-holding-account-short.json` | 25 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-a-holding-sum-past-the-bound.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-a-report-difference.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-a-wrong-amount-at-a-public-end.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-reports-in-two-assets.json` | 41 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-reports-of-one-amount-in-two-units.json` | 41 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/raise-of-value-evidenced-at-an-unread-account.json` | 19 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/read-answer-allowed.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/read-answer-refused.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/read-check-by-a-party-of-its-own-evidence.json` | 17 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/read-check-by-a-provider-of-its-segment.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/read-check-by-a-regulator-of-the-whole-trail.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/read-check-by-an-agent-in-its-partys-role.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/read-check-of-who-the-checker-is.json` | 14 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/receipt-of-an-appended-entry.json` | 8 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/report-pair-filled-in-increments.json` | 44 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/report-pair-of-a-route-segment.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/report-pair-reported-gross.json` | 44 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/session-answer-opened.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/session-answer-refused.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/session-request-of-a-delegate.json` | 13 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/stated-entry-of-a-publishers-record-of-a-nodes-segments.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/stated-entry-of-a-quote.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/stated-entry-relayed-and-unverified.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/stated-entry-relayed-by-a-sibling-of-its-publisher.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/stuck-decide-handing-value-on-to-a-linked-order.json` | 29 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/stuck-decide-naming-its-evidence.json` | 39 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/stuck-move-carrying-out-a-decision.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-chained-decision-record.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-charge-counted-within-its-bound.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-charge-not-known.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-charge-over-its-bound.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-customers-permitted-read-of-its-order.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-decision.json` | 52 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-delivered-raise.json` | 52 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-destination-whose-owner-changed.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-fee-billed-separately.json` | 29 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-fee-its-provider-said-nothing-of-billing.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-first-decision-record.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-hand-over.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-holding-check.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-holding-withdrawal-labelled-its-publishers-word.json` | 32 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-holding-withdrawal.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-late-approval.json` | 29 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-measure.json` | 45 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-notice-summary-delivered.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-notice-summary-of-a-raise-abandoned.json` | 29 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-permission-granted.json` | 40 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-permission-revoked.json` | 40 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-public-mismatch-of-a-wrong-amount.json` | 47 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-public-mismatch-of-a-wrong-destination.json` | 47 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-question-past-its-bound.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-quote-shown-without-a-firm-rate.json` | 25 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-recorded-input-destination-changed-with-acceptance.json` | 80 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-recorded-input-handed-on.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-recorded-input-route-chosen-three-parts.json` | 236 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-recorded-input.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-refusal-with-nothing-existing.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-refused-read.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-registered-authorisation.json` | 44 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-regulators-permitted-read.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-replay-consistent-with-its-exit.json` | 42 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-replay-mismatch-at-the-second-node.json` | 48 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-replay-of-a-node-that-recorded-nothing.json` | 40 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-replay-resting-on-its-nodes-records.json` | 58 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-report-pair.json` | 62 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-result-unanswered.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-reversal-carried-by-its-provider.json` | 22 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-settlement-time.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-shortlist-record-keeping-no-provider.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-shortlist-record-naming-the-verifiers-it-needs.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-stuck-decision.json` | 59 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-stuck-move-by-the-deciders-party.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-stuck-parts-evidence.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-acceptance-by-automatic-deposit.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-acceptance-of-a-route-handed-on.json` | 49 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-acceptance-of-a-split-order.json` | 61 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-acceptance-paying-a-settlement-account.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-agents-acceptance.json` | 49 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-arrival-filed-anew-by-its-reserve.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-arrival-naming-its-earlier-arrival.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-arrival-that-differs.json` | 42 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-effect.json` | 54 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-erasure.json` | 23 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-input-not-taken.json` | 41 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-opened-session.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-operation-that-ended-with-no-answer.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-operator-command.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-order-waiting-on-its-destinations-new-owner.json` | 22 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-outcome-confirmed.json` | 56 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-outcome-report-declined.json` | 55 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-outcome-reported.json` | 48 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-graded-evidence-naming-its-public-movement.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-graded-evidence.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-reliance-on-a-judgement.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-signed-evidence-naming-the-movement-core-checked.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-the-figures-behind-a-stuck-call.json` | 25 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-the-operators-read-of-an-orders-state.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/schemas/authorisation.schema.json` | 63 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/charge-answer.schema.json` | 177 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/charge.schema.json` | 50 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/checkpoint-opening.schema.json` | 37 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/checkpoint.schema.json` | 73 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/common.schema.json` | 643 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/decision-request.schema.json` | 46 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/decision.schema.json` | 143 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/effect.schema.json` | 57 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/entry.schema.json` | 50 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/handover.schema.json` | 660 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/holding-reading.schema.json` | 77 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/kept-statement.schema.json` | 30 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/measure-record.schema.json` | 288 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/permission-change.schema.json` | 42 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/person-answer.schema.json` | 172 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/person-question.schema.json` | 163 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/raise.schema.json` | 510 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/read-answer.schema.json` | 105 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/read-check.schema.json` | 257 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/receipt.schema.json` | 27 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/report-pair.schema.json` | 122 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/session-answer.schema.json` | 148 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/session-request.schema.json` | 42 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/stated-entry.schema.json` | 238 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/stuck-decide.schema.json` | 169 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/stuck-move.schema.json` | 37 | 0 | sound (closed objects, references) |
| `contracts/core-internal/schemas/verified-entry.schema.json` | 2892 | 0 | sound (closed objects, references) |
| `contracts/provider-role/CHANGELOG.md` | 111 | 0 | docs; sound |
| `contracts/provider-role/README.md` | 159 | 3 | docs; sound |
| `contracts/provider-role/fixtures/invalid/answered-arrival-question-about-another-account.json` | 81 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/answered-arrival-question-at-another-node.json` | 81 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/answered-arrival-question-by-another-reserve.json` | 81 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/answered-arrival-question-in-another-asset.json` | 81 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/answered-arrival-question-in-another-unit.json` | 81 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/answered-arrival-question-under-another-transaction.json` | 81 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-answer-arrived-with-no-notices.json` | 22 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-answer-carrying-another-reserves-notice.json` | 51 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-answer-carrying-another-shape-as-its-notice.json` | 51 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-answer-refusal-saying-when-it-lifts.json` | 25 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-answer-that-nothing-arrived-carrying-a-notice.json` | 51 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-answer-under-another-shape.json` | 51 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-answer-without-its-transaction.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-asset-without-its-unit.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-at-an-inner-node.json` | 26 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-of-a-zero-amount.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-under-another-shape.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-without-its-account.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-without-its-amount.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-without-its-asset.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-without-its-holder.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-without-its-outer-node.json` | 25 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notice-without-its-transaction.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-notices-of-one-credit-with-another-amount.json` | 58 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-question-at-an-inner-node.json` | 26 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-question-signed-with-another-key.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-question-under-another-shape.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-question-without-a-session.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/arrival-question-without-its-transaction.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/hold-for-no-order-or-session.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/hold-of-a-record-without-its-digest.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/hold-without-its-transaction.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-fee-with-a-fraction.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-labelled-as-proof.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-segment-without-its-amount-in.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-segment-without-its-amount-out.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-segment-without-its-asset-in.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-segment-without-its-fee.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-segment-without-its-transaction.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-under-another-shape.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-without-its-label.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-without-its-publisher.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/inner-segments-without-segments.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/published-inner-segments-against-another-providers-advertisement.json` | 109 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/published-inner-segments-at-a-node-its-maker-does-not-declare.json` | 109 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/published-inner-segments-for-another-publisher.json` | 109 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/result-transfer-failed-showing-the-providers-own-fault.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/result-withdrawal-billing-its-fee-separately-as-text.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/terms-made-by-another-party.json` | 46 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/terms-of-a-reserve-with-a-slippage-regime.json` | 46 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/terms-of-an-exchange-without-a-slippage-regime.json` | 42 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/terms-under-another-shape.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/terms-with-a-zero-denominator.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/answered-arrival-question-by-the-notice-of-its-credit.json` | 81 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/answered-arrival-question-that-nothing-arrived.json` | 51 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/arrival-answer-carrying-the-notice-of-a-credit.json` | 51 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/arrival-answer-refused-waiting-on-the-person.json` | 24 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/arrival-answer-that-nothing-arrived.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/arrival-notice-of-a-credit.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/arrival-notices-of-one-credit-signed-at-different-times.json` | 58 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/arrival-notices-of-one-reference-at-two-reserves.json` | 58 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/arrival-question-of-what-arrived-at-an-account.json` | 28 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/hold-for-a-session.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/hold-for-an-order.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/hold-of-another-records-candidate-of-the-same-id.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/inner-segments-of-a-conversion-and-a-transfer.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/inner-segments-that-do-not-add-up.json` | 38 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/published-inner-segments-of-a-node-its-maker-declares.json` | 109 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/result-transfer-failed-showing-the-customers-fault.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/result-withdrawal-succeeded-billing-its-fee-separately.json` | 36 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/terms-of-a-reserve-withdrawal.json` | 43 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/terms-of-an-exchange-with-its-slippage-regime.json` | 46 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/schemas/arrival-answer.schema.json` | 132 | 0 | sound (closed objects, references) |
| `contracts/provider-role/schemas/arrival-notice.schema.json` | 56 | 0 | sound (closed objects, references) |
| `contracts/provider-role/schemas/arrival-question.schema.json` | 56 | 0 | sound (closed objects, references) |
| `contracts/provider-role/schemas/hold.schema.json` | 123 | 0 | sound (closed objects, references) |
| `contracts/provider-role/schemas/inner-segments.schema.json` | 89 | 0 | sound (closed objects, references) |
| `contracts/provider-role/schemas/result.schema.json` | 11 | 0 | sound (closed objects, references) |
| `contracts/provider-role/schemas/terms.schema.json` | 118 | 0 | sound (closed objects, references) |
| `contracts/router-api/CHANGELOG.md` | 79 | 0 | docs; sound |
| `contracts/router-api/README.md` | 77 | 7 | docs; sound |
| `contracts/router-api/fixtures/invalid/decision-record-below-floor-for-a-candidate-that-arrives-above-it.json` | 1948 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-below-floor-for-a-candidate-the-operator-s-fee-leaves-above-it.json` | 1948 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-below-floor-without-a-floor.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-beyond-levels-naming-an-operation-before-the-last.json` | 70 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-beyond-levels-with-something-out.json` | 70 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-beyond-levels-within-the-levels.json` | 70 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-candidate-id-twice.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-candidate-without-a-drop-reason.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-choice-leaving-a-judgement-of-no-uncited.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-choosing-a-gross-deposit-stated-as-its-answer.json` | 2995 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-choosing-an-unconfirmed-candidate.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-below-the-floor-after-the-operator-s-fee.json` | 2195 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-below-the-floor.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-citing-a-judgement-of-no.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-outside-a-band.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-over-a-ceiling-on-what-it-produces.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-over-a-ceiling.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-over-the-hop-limit.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-using-a-stale-figure.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-using-a-void-statement.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-using-an-excluded-provider.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-candidate-with-a-drop-reason.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-not-a-candidate.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-null.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-operation-claiming-a-lower-fee-than-its-quote.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-chosen-operation-claiming-more-than-its-quote-delivers.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-citing-a-calls-figure-and-naming-no-call.json` | 11 | 2 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-confirmed-by-an-answer-made-before-the-shortlist.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-drop-reason-naming-a-figure-its-candidate-does-not-cite.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-dry-run-cited-at-another-amount.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-dry-run-refused-on-an-accepted-dry-run.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-excluded-without-an-exclusion.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-fee-above-amount-on-an-operation-a-usable-answer-covers.json` | 90 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-fee-above-amount-stating-a-lower-fee.json` | 90 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-fee-above-amount-with-something-out.json` | 90 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-ineligible-judgement-moved-to-the-withdrawal.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-ineligible-on-a-yes-judgement.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-judgement-of-no-between-the-shortlist-and-the-answers-left-uncited.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-judgement-of-no-made-before-the-shortlist-left-uncited.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-naming-a-call-to-a-provider-off-its-part.json` | 11 | 2 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-naming-a-call-without-its-calls-figure.json` | 11 | 2 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-naming-an-undeclared-call.json` | 11 | 2 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-naming-calls-of-another-release.json` | 11 | 2 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-no-positive-outcome-for-a-candidate-that-arrives.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-no-price-for-a-tuple-the-snapshot-prices.json` | 90 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-no-price-naming-an-operation-before-the-last.json` | 90 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-no-price-stating-a-fee.json` | 90 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-nothing-delivered-at-the-destination.json` | 70 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-nothing-delivered-naming-an-operation-before-the-last.json` | 70 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-nothing-delivered-with-something-out.json` | 70 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-of-another-shape.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-operation-citing-a-missing-figure.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-operation-citing-a-quote-at-another-amount.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-operation-citing-a-tuple-with-other-ends.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-operation-citing-another-providers-tuple.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-operation-citing-no-price.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-operation-citing-no-tuple.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-outranked-by-a-missing-candidate.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-outside-band-for-an-operation-within-it.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-over-a-ceiling-at-other-decimals.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-over-a-ceiling-it-stays-under.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-over-a-ceiling-the-bounds-lack.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-part-not-ending-at-the-destination.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-part-not-starting-at-the-origin.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-parts-not-adding-up.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-price-of-another-tuple.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-quote-voided-by-a-condition-it-does-not-name.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-quote-voided-by-an-unchanged-amount.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-quote-voided-citing-a-second-quote-at-another-amount.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-quote-voided-on-an-operation-a-usable-answer-covers.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-ranking-net-of-the-operator-s-fee.json` | 2194 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-refining-a-shortlist-taken-after-its-snapshot.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-refused-dry-run-moved-to-the-deposit.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-shortlist-round-choosing.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-shortlisted-candidate-citing-a-refused-dry-run.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-shortlisted-candidate-with-a-part-arriving-with-nothing.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-stale-reason-naming-a-fresh-figure.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-stale-reason-naming-a-missing-figure.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-stale-reason-on-an-operation-a-usable-answer-covers.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-unconfirmed-in-a-shortlist-round.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-unconfirmed-leaving-its-quote-uncited.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-unconfirmed-naming-a-confirmed-operation.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-usable-answers-for-one-amount-disagreeing.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-void-reason-naming-a-statement-valid-here.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-without-a-slippage-limit-for-an-exchange-settling-within-three-days.json` | 2153 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-without-envelope.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-without-router-version.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-without-snapshot.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/held-quote-of-a-record-choosing-nothing.json` | 1936 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/held-quote-valid-until-before-its-snapshot.json` | 2182 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/held-quote-valid-until-past-its-quotes-figures.json` | 2182 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/held-quote-without-validity.json` | 2181 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-excluding-a-provider-twice.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-operator-fee-above-the-whole.json` | 62 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-slippage-limit-beyond-ten-thousand-bps.json` | 62 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-two-ceilings-for-one-provider-and-asset.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-two-slippage-limits-for-one-tuple.json` | 82 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-with-no-hops.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-without-ceilings.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-without-deployment.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-without-operator-fee.json` | 54 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-without-protocol-version.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/policy-bounds-without-slippage-limits.json` | 40 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/settlement-times-settled-before-started.json` | 5 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/settlement-times-without-declared-seconds.json` | 4 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/stated-terms-without-a-tuple.json` | 10 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/stated-terms-without-stated-at.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-below-floor-for-a-candidate-that-arrives-above-it.json` | 10 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-below-floor-for-a-candidate-the-operator-s-fee-leaves-above-it.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-choosing-a-gross-deposit-stated-as-its-answer.json` | 10 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-chosen-candidate-below-the-floor-after-the-operator-s-fee.json` | 21 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-ranking-net-of-the-operator-s-fee.json` | 10 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-without-a-slippage-limit-for-an-exchange-settling-within-three-days.json` | 10 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/catalogue-snapshot-judgements-of-two-classes-disagreeing.json` | 1676 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-choosing-a-gross-deposit-stated-as-what-arrives.json` | 2995 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-kept-at-the-floor-after-a-capped-operator-s-fee.json` | 2195 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-naming-the-calls-each-segment-makes.json` | 11 | 2 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-over-a-ceiling-on-what-it-produces.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-shortlist-round.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-shortlist-with-a-route-beyond-a-price-s-levels.json` | 70 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-shortlist-with-a-route-that-delivers-nothing-before-the-destination.json` | 70 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-shortlist-with-a-tuple-of-no-price-and-a-fee-above-its-amount.json` | 90 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-split-of-one-part.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-split-of-three-parts.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-split-with-a-part-arriving-with-nothing.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-with-every-drop-reason-of-the-choice-round.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-with-no-route.json` | 50 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/held-quote-of-a-choice-round.json` | 2182 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/policy-bounds-excluding-a-provider-and-a-tuple.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/policy-bounds-with-an-operator-fee-and-slippage-limits.json` | 62 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/policy-bounds-with-ceilings-in-two-units.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/policy-bounds-without-splitting.json` | 9 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/settlement-times-of-a-segment.json` | 5 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/stated-terms-of-a-ramp.json` | 27 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/stated-terms-of-an-exchange.json` | 31 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/schemas/candidate.schema.json` | 1 | 1 | sound (closed objects, references) |
| `contracts/router-api/schemas/decision-record.schema.json` | 7 | 0 | sound (closed objects, references) |
| `contracts/router-api/schemas/held-quote.schema.json` | 22 | 0 | sound (closed objects, references) |
| `contracts/router-api/schemas/policy-bounds.schema.json` | 31 | 2 | sound (closed objects, references) |
| `contracts/router-api/schemas/settlement-times.schema.json` | 27 | 0 | sound (closed objects, references) |
| `contracts/router-api/schemas/stated-terms.schema.json` | 30 | 0 | sound (closed objects, references) |
| `contracts/src/core_internal/attribution.rs` | 517 | 0 | sound; residuals R-BACKDATE, R-FUTURE |
| `contracts/src/core_internal/calls.rs` | 403 | 0 | sound (two handles; residual R-HANDLES) |
| `contracts/src/core_internal/check.rs` | 814 | 0 | sound |
| `contracts/src/core_internal/mod.rs` | 111 | 0 | sound |
| `contracts/src/core_internal/relations.rs` | 6215 | 0 | sound on the rows read (permits, stuck, holds, arrivals, erasure); residuals R-NODE, R-RELIANCE, R-HOLDER |
| `contracts/src/core_internal/relations/closing.rs` | 344 | 0 | sound; residual R-NODE |
| `contracts/src/core_internal/standing.rs` | 302 | 0 | sound |
| `contracts/src/core_internal/types.rs` | 3203 | 0 | sound |
| `contracts/src/lib.rs` | 1 | 0 | sound |
| `contracts/src/provider_role/arrival.rs` | 283 | 0 | sound |
| `contracts/src/provider_role/inner.rs` | 113 | 0 | sound |
| `contracts/src/provider_role/mod.rs` | 27 | 2 | sound |
| `contracts/src/provider_role/outcome.rs` | 26 | 0 | sound |
| `contracts/src/provider_role/settlement.rs` | 99 | 2 | sound |
| `contracts/src/provider_role/stated.rs` | 238 | 0 | sound |
| `contracts/src/provider_role/types.rs` | 2 | 1 | sound |
| `contracts/src/router_api/atoms.rs` | 101 | 3 | sound |
| `contracts/src/router_api/check.rs` | 336 | 24 | sound |
| `contracts/src/router_api/mod.rs` | 11 | 2 | sound |
| `contracts/src/router_api/types.rs` | 112 | 3 | sound |
| `contracts/src/state_machine/host/mod.rs` | 1 | 1 | sound |
| `contracts/src/state_machine/mod.rs` | 164 | 9 | sound |
| `contracts/src/state_machine/rules.rs` | 568 | 0 | sound |
| `contracts/state-machine/CHANGELOG.md` | 49 | 0 | docs; sound |
| `contracts/state-machine/README.md` | 46 | 0 | docs; sound |
| `contracts/state-machine/fixtures/invalid/order-failed-final-deposit-without-deposit-failed-at.json` | 2 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/order-part-deposit-failed-at-an-outer-node.json` | 2 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/order-part-deposit-failed-at-null.json` | 2 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/order-resolved-but-still-in-progress.json` | 96 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/order-resolved-to-its-origin-but-complete.json` | 96 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/order-waiting-for-the-customer-without-waiting-since.json` | 95 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/order-waiting-since-while-not-waiting.json` | 96 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/recorded-input-stuck-resolved-naming-a-segment.json` | 20 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/recorded-input-stuck-resolved-somewhere-else.json` | 19 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/recorded-input-stuck-resolved-without-evidence.json` | 18 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/recorded-input-waiting-at-floor-carrying-an-amount.json` | 18 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/invalid/recorded-input-waiting-at-floor-naming-a-segment.json` | 18 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/valid/order-delivered-by-its-stuck-resolution.json` | 96 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/valid/order-returned-to-its-origin-by-its-stuck-resolution.json` | 96 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/valid/order-waiting-for-the-customer-after-a-failed-final-deposit.json` | 2 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/valid/order-waiting-for-the-customer-at-its-floor.json` | 96 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/valid/recorded-input-hand-on-taken-back.json` | 18 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/valid/recorded-input-stuck-resolved-to-the-destination.json` | 19 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/valid/recorded-input-stuck-resolved-to-the-origin.json` | 19 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/fixtures/valid/recorded-input-waiting-at-floor.json` | 17 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/CHANGELOG.md` | 8 | 0 | docs; sound |
| `contracts/state-machine/host/fixtures/invalid/snapshot-stream-cut-short-before-its-end.jsonl` | 1 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/fixtures/invalid/snapshot-stream-with-a-gap-in-a-sequence.jsonl` | 1 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/fixtures/invalid/snapshot-stream-with-a-record-id-twice.jsonl` | 1 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/fixtures/invalid/snapshot-stream-with-a-refusal-pointing-elsewhere.jsonl` | 1 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/fixtures/invalid/snapshot-stream-with-an-end-count-that-differs.jsonl` | 1 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/fixtures/invalid/snapshot-stream-with-an-entry-after-its-end.jsonl` | 2 | 2 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/fixtures/valid/snapshot-entry-header.json` | 2 | 2 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/fixtures/valid/snapshot-stream-of-an-empty-host.jsonl` | 1 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/host/fixtures/valid/snapshot-stream-of-orders-in-flight.jsonl` | 1 | 1 | data; checked by `contracts/tests/`; no key material |
| `contracts/state-machine/schemas/order-state.schema.json` | 1 | 1 | sound (closed objects, references) |
| `contracts/state-machine/schemas/order.schema.json` | 73 | 0 | sound (closed objects, references) |
| `contracts/state-machine/schemas/recorded-input.schema.json` | 68 | 1 | sound (closed objects, references) |
| `contracts/tests/core-internal.rs` | 4033 | 0 | test; sound |
| `contracts/tests/provider-role.rs` | 704 | 7 | test; sound |
| `contracts/tests/router-api.rs` | 254 | 8 | test; sound |
| `contracts/tests/state-machine.rs` | 97 | 3 | test; sound |
| `licences/RECORD` | 27 | 0 | sound (permissive only) |
| `tools/boundary-check/Cargo.toml` | 3 | 0 | sound (no new crate of ours; boundary holds) |
| `tools/boundary-check/src/lib.rs` | 529 | 8 | sound |
| `tools/boundary-check/tests/boundary.rs` | 405 | 1 | test; sound |
| `tools/boundary-check/tests/graph.rs` | 113 | 0 | sound (only the assembly reaches core-audit and core-authz) |
| `tools/contract-guard/src/lib.rs` | 233 | 0 | sound (stricter: a referenced major moves referencing majors) |
| `tools/contract-guard/tests/guard.rs` | 180 | 0 | test; sound |

## Second pass (`e63afbeb..2caa6f55`, 2026-11-24)

The paths `git diff --name-only e63afbeb..2caa6f55` lists under a `DEC-20` prefix, with `conformance/evidence/v0-scope`:
92 paths, 33 of them code, schemas, docs and tests, and 59 contract fixtures, all new. No workflow,
`components/prov/`, `tools/` or `licences/` path changed. Ids are the findings and residuals of `notes.md`, section
"T-025-05, second pass". Counts are lines added and removed.

| Path | + | - | Verdict |
|---|---|---|---|
| `components/core-audit/src/log/keys.rs` | 93 | 10 | sound (the received-time rule and its clock hold, `T-022-09`); low L5; residuals R-BACKDATE (narrowed), R-FUTURE, R-KEYS |
| `components/core-audit/src/receipts/store.rs` | 175 | 14 | sound (the override read from the permitting decision; later citations reported, `T-022-10`); residual R-OVERRIDE |
| `components/core-audit/tests/log.rs` | 257 | 6 | test; sound |
| `components/core-audit/tests/receipts.rs` | 174 | 3 | test; sound |
| `components/core-authz/src/engine/entry_points.rs` | 51 | 5 | sound (`roll_back` carries out; its planted bypass caught); informational note on `carry_out_roll_back` |
| `components/core-authz/src/engine/mod.rs` | 16 | 4 | sound (17 decided actions; `AC-17`'s note settled) |
| `components/core-authz/src/funds/mod.rs` | 34 | 29 | sound (the origin held once set, `T-021-11`) |
| `components/core-authz/src/funds/tests.rs` | 152 | 1 | test; sound |
| `components/core-authz/src/roles/mod.rs` | 30 | 1 | sound (roll back's own permission; the override needs both permissions, operator only) |
| `components/core-authz/src/roles/tests.rs` | 300 | 3 | test; sound |
| `conformance/evidence/v0-scope` | 1 | 1 | sound (`REQ-CAT-5` to `m3` on the owner's acceptance of 2026-11-18; one `strict:` line, `on`) |
| `contracts/CHANGELOG.md` | 7 | 0 | sound |
| `contracts/core-internal/CHANGELOG.md` | 54 | 0 | sound |
| `contracts/core-internal/README.md` | 15 | 11 | sound |
| `contracts/core-internal/schemas/common.schema.json` | 7 | 2 | sound |
| `contracts/core-internal/schemas/verified-entry.schema.json` | 92 | 6 | sound (`awaiting`, `waiting`); residual R-AWAITING |
| `contracts/provider-role/CHANGELOG.md` | 7 | 0 | sound |
| `contracts/provider-role/schemas/advertisement.schema.json` | 7 | 0 | sound (`payouts`, deposit tuples only by the Rust rule) |
| `contracts/router-api/CHANGELOG.md` | 16 | 0 | sound |
| `contracts/router-api/schemas/candidate.schema.json` | 34 | 0 | sound (`batched_payout`) |
| `contracts/src/core_internal/attribution.rs` | 70 | 2 | sound (`revoked_before_receipt`); low L5 (done changes only); residual R-BACKDATE |
| `contracts/src/core_internal/calls.rs` | 19 | 7 | sound (`signs_for` takes the received time) |
| `contracts/src/core_internal/check.rs` | 35 | 1 | sound (`given_on_deposit`'s shape; `awaiting` exactly on a slow quote) |
| `contracts/src/core_internal/mod.rs` | 1 | 1 | sound |
| `contracts/src/core_internal/relations.rs` | 227 | 21 | sound (the party rule; the override from the permitting decision; deposits' acceptance); low L7; residuals R-MOVER, R-HOLDER |
| `contracts/src/core_internal/types.rs` | 85 | 4 | sound |
| `contracts/src/provider_role/types.rs` | 29 | 0 | sound |
| `contracts/src/provider_role/validate.rs` | 10 | 1 | sound |
| `contracts/src/router_api/check.rs` | 45 | 0 | sound (the candidate's half bounds the index the record's half reads) |
| `contracts/src/router_api/types.rs` | 10 | 0 | sound |
| `contracts/tests/core-internal.rs` | 511 | 4 | test; sound |
| `contracts/tests/provider-role.rs` | 90 | 0 | test; sound |
| `contracts/tests/router-api.rs` | 90 | 1 | test; sound |
| `contracts/core-internal/fixtures/admission/acceptance-is-asked-in-the-users-session--given-on-a-deposit-in-no-session.json` | 3732 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/acceptance-is-of-the-quote-shown--given-on-a-deposit-of-a-quote-not-shown.json` | 3732 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/automatic-deposit-rests-on-its-arrival--given-on-a-deposit-with-no-arrival.json` | 3732 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/automatic-deposit-rests-on-its-arrival--given-on-a-deposit.json` | 3731 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-waits-while-anything-rests-on-the-statement--a-decision-overriding-the-citations.json` | 3036 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-waits-while-anything-rests-on-the-statement--an-effect-on-a-decision-overriding-the-citations.json` | 3142 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-waits-while-anything-rests-on-the-statement--an-effect-overriding-on-a-decision-that-does-not.json` | 3142 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/erasure-waits-while-anything-rests-on-the-statement--an-erasure-on-a-decision-overriding-the-citations.json` | 3157 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/first-move-rests-on-its-parts-yes--given-on-a-deposit-by-anyone.json` | 1418 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/first-move-rests-on-its-parts-yes--given-on-a-deposit.json` | 1417 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-a-holding-check-names.json` | 4210 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-of-a-route-handed-on-from-the-order.json` | 4248 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-of-an-order-a-hand-on-input-joins.json` | 4467 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-of-an-order-a-holding-withdrawal-joins.json` | 4460 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-of-an-order-a-permitted-decide-hands-on-to.json` | 4478 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-of-an-order-a-refused-decide-hands-on-to.json` | 4480 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-of-an-order-a-resolve-effect-hands-on-to.json` | 4480 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-of-an-order-no-hand-on-joins.json` | 4420 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-only-a-recalculated-route-names.json` | 4262 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-another-provider-of-its-route.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-the-holder-a-holding-withdrawal-names.json` | 4214 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-the-mover-a-permitted-decide-names.json` | 4238 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-the-mover-a-refused-decide-names.json` | 4240 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-the-mover-its-resolve-effect-names.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-the-publisher-a-holding-withdrawal-names.json` | 4215 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-confirmation-on-the-reserve-its-arrival-came-to.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-decline-on-an-outside-providers-statement.json` | 4173 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--a-move-values-report-on-an-outside-providers-statement.json` | 3834 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/outcome-confirmation-cites-verified-evidence--party.json` | 4174 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/admission/part-evidence-rests-on-statements--party.json` | 3778 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-statement-made-before-a-revocation-received-after-its-grant-was-made-again.json` | 322 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-statement-received-after-its-grants-revocation.json` | 234 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/unverified--a-statement-received-at-its-grants-revocation.json` | 234 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-statement-made-after-its-grant-was-made-again.json` | 322 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-statement-received-after-one-grants-revocation-while-another-stands.json` | 322 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/attribution/verified--a-statement-received-before-its-grants-revocation.json` | 234 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/decision-request-to-roll-back-naming-no-version.json` | 15 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-quote-awaiting-its-customer-naming-a-segment-twice.json` | 37 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-quote-awaiting-its-customer-shown-with-a-firm-rate.json` | 39 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-a-quote-awaiting-its-customer-without-a-firm-rate-naming-no-slow-segment.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-acceptance-given-on-a-deposit-naming-a-yes.json` | 49 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/invalid/verified-entry-of-an-acceptance-given-on-a-deposit-without-its-session.json` | 34 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/decision-request-to-roll-back-a-staged-release.json` | 16 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-notice-summary-of-a-waiting-order-delivered.json` | 33 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-quote-awaiting-its-customer-asked-again-firm.json` | 30 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-a-quote-awaiting-its-customer.json` | 39 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/core-internal/fixtures/valid/verified-entry-of-an-acceptance-given-on-a-deposit.json` | 35 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/advertisement-deposit-tuple-declaring-payouts-of-no-kind.json` | 95 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/invalid/advertisement-withdrawal-tuple-declaring-payouts.json` | 95 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/provider-role/fixtures/valid/advertisement-reserve-paying-out-deposits-in-batches.json` | 95 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-batched-payout-at-a-tuple-paying-per-order.json` | 744 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-batched-payout-naming-an-earlier-deposit-at-a-batching-reserve.json` | 864 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-batched-payout-naming-an-operation-before-the-last.json` | 745 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/invalid/decision-record-batching-deposit-ending-a-kept-candidate.json` | 740 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-batched-payout-at-a-tuple-paying-per-order.json` | 9 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-batched-payout-naming-an-earlier-deposit-at-a-batching-reserve.json` | 259 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-batched-payout-naming-an-operation-before-the-last.json` | 10 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/patches/decision-record-batching-deposit-ending-a-kept-candidate.json` | 9 | 0 | data; checked by `contracts/tests/`; no key material |
| `contracts/router-api/fixtures/valid/decision-record-shortlist-dropping-a-route-ending-at-a-batching-reserve.json` | 745 | 0 | data; checked by `contracts/tests/`; no key material |

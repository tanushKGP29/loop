# Decision-model evaluation

Human labels were validated before requests were sent.

## laya

### tuning

| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 0.0% | 100.0% | 100.0% | 0.0% | 2147.0 ms | 2147 ms |

#### Tool reliability by confidence band

| band | cases | mean confidence | empirical accuracy |
| --- | ---: | ---: | ---: |
| [0.00, 0.50) | 1 | 32.0% | 0.0% |

#### Tool threshold candidates

| threshold | coverage | cases | tool accuracy |
| ---: | ---: | ---: | ---: |
| 0.50 | 0.0% | 0 | — |
| 0.60 | 0.0% | 0 | — |
| 0.70 | 0.0% | 0 | — |
| 0.75 | 0.0% | 0 | — |
| 0.80 | 0.0% | 0 | — |
| 0.90 | 0.0% | 0 | — |
| 0.95 | 0.0% | 0 | — |

## Interpretation guardrails

- Select a production threshold from the tuning rows, then report the held-out result unchanged.
- Do not claim calibration from a sparse band. Each reported band should have enough examples to be meaningful.
- The Soket baseline returns self-reported confidence through prompted JSON; compare its accuracy and latency, but do not treat its confidence as calibrated like a System One probability.

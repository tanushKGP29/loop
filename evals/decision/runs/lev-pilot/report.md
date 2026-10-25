# Decision-model evaluation

Human labels were validated before requests were sent.

## lev

### tuning

| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 100.0% | 100.0% | 100.0% | 100.0% | 29106.0 ms | 29106 ms |

#### Tool reliability by confidence band

| band | cases | mean confidence | empirical accuracy |
| --- | ---: | ---: | ---: |
| [0.70, 0.90) | 1 | 86.6% | 100.0% |

#### Tool threshold candidates

| threshold | coverage | cases | tool accuracy |
| ---: | ---: | ---: | ---: |
| 0.50 | 100.0% | 1 | 100.0% |
| 0.60 | 100.0% | 1 | 100.0% |
| 0.70 | 100.0% | 1 | 100.0% |
| 0.75 | 100.0% | 1 | 100.0% |
| 0.80 | 100.0% | 1 | 100.0% |
| 0.90 | 0.0% | 0 | — |
| 0.95 | 0.0% | 0 | — |

## Interpretation guardrails

- Select a production threshold from the tuning rows, then report the held-out result unchanged.
- Do not claim calibration from a sparse band. Each reported band should have enough examples to be meaningful.
- The Soket baseline returns self-reported confidence through prompted JSON; compare its accuracy and latency, but do not treat its confidence as calibrated like a System One probability.

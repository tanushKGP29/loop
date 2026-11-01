# Decision-model evaluation

Human labels were validated before requests were sent.

## lev

### tuning

| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 30 | 83.3% | 90.0% | 90.0% | 70.0% | 26797.2 ms | 31416 ms |

#### Tool reliability by confidence band

| band | cases | mean confidence | empirical accuracy |
| --- | ---: | ---: | ---: |
| [0.00, 0.50) | 13 | 29.2% | 69.2% |
| [0.50, 0.70) | 5 | 55.2% | 80.0% |
| [0.70, 0.90) | 7 | 82.7% | 100.0% |
| [0.90, 1.00] | 5 | 93.5% | 100.0% |

#### Tool threshold candidates

| threshold | coverage | cases | tool accuracy |
| ---: | ---: | ---: | ---: |
| 0.50 | 56.7% | 17 | 94.1% |
| 0.60 | 43.3% | 13 | 100.0% |
| 0.70 | 40.0% | 12 | 100.0% |
| 0.75 | 36.7% | 11 | 100.0% |
| 0.80 | 36.7% | 11 | 100.0% |
| 0.90 | 16.7% | 5 | 100.0% |
| 0.95 | 3.3% | 1 | 100.0% |

### holdout

| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 10 | 90.0% | 80.0% | 90.0% | 70.0% | 25146.6 ms | 27027 ms |

#### Tool reliability by confidence band

| band | cases | mean confidence | empirical accuracy |
| --- | ---: | ---: | ---: |
| [0.00, 0.50) | 6 | 24.2% | 83.3% |
| [0.50, 0.70) | 1 | 68.7% | 100.0% |
| [0.70, 0.90) | 1 | 79.8% | 100.0% |
| [0.90, 1.00] | 2 | 91.7% | 100.0% |

#### Tool threshold candidates

| threshold | coverage | cases | tool accuracy |
| ---: | ---: | ---: | ---: |
| 0.50 | 40.0% | 4 | 100.0% |
| 0.60 | 40.0% | 4 | 100.0% |
| 0.70 | 30.0% | 3 | 100.0% |
| 0.75 | 30.0% | 3 | 100.0% |
| 0.80 | 20.0% | 2 | 100.0% |
| 0.90 | 20.0% | 2 | 100.0% |
| 0.95 | 0.0% | 0 | — |

## Interpretation guardrails

- Select a production threshold from the tuning rows, then report the held-out result unchanged.
- Do not claim calibration from a sparse band. Each reported band should have enough examples to be meaningful.
- The Soket baseline returns self-reported confidence through prompted JSON; compare its accuracy and latency, but do not treat its confidence as calibrated like a System One probability.

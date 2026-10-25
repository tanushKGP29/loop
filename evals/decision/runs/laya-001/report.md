# Decision-model evaluation

Human labels were validated before requests were sent.

## laya

### tuning

| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 30 | 40.0% | 63.3% | 70.0% | 6.7% | 1280.7 ms | 1625 ms |

#### Tool reliability by confidence band

| band | cases | mean confidence | empirical accuracy |
| --- | ---: | ---: | ---: |
| [0.00, 0.50) | 21 | 29.2% | 33.3% |
| [0.50, 0.70) | 7 | 57.4% | 57.1% |
| [0.70, 0.90) | 1 | 77.5% | 100.0% |
| [0.90, 1.00] | 1 | 92.9% | 0.0% |

#### Tool threshold candidates

| threshold | coverage | cases | tool accuracy |
| ---: | ---: | ---: | ---: |
| 0.50 | 30.0% | 9 | 55.6% |
| 0.60 | 13.3% | 4 | 50.0% |
| 0.70 | 6.7% | 2 | 50.0% |
| 0.75 | 6.7% | 2 | 50.0% |
| 0.80 | 3.3% | 1 | 0.0% |
| 0.90 | 3.3% | 1 | 0.0% |
| 0.95 | 0.0% | 0 | — |

### holdout

| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 10 | 50.0% | 30.0% | 60.0% | 0.0% | 1493.0 ms | 2094 ms |

#### Tool reliability by confidence band

| band | cases | mean confidence | empirical accuracy |
| --- | ---: | ---: | ---: |
| [0.00, 0.50) | 8 | 34.0% | 50.0% |
| [0.70, 0.90) | 1 | 75.4% | 0.0% |
| [0.90, 1.00] | 1 | 91.7% | 100.0% |

#### Tool threshold candidates

| threshold | coverage | cases | tool accuracy |
| ---: | ---: | ---: | ---: |
| 0.50 | 20.0% | 2 | 50.0% |
| 0.60 | 20.0% | 2 | 50.0% |
| 0.70 | 20.0% | 2 | 50.0% |
| 0.75 | 20.0% | 2 | 50.0% |
| 0.80 | 10.0% | 1 | 100.0% |
| 0.90 | 10.0% | 1 | 100.0% |
| 0.95 | 0.0% | 0 | — |

## Interpretation guardrails

- Select a production threshold from the tuning rows, then report the held-out result unchanged.
- Do not claim calibration from a sparse band. Each reported band should have enough examples to be meaningful.
- The Soket baseline returns self-reported confidence through prompted JSON; compare its accuracy and latency, but do not treat its confidence as calibrated like a System One probability.

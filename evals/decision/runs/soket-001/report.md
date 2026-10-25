# Decision-model evaluation

Human labels were validated before requests were sent.

## soket_plain_llm

### tuning

| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 29 | 93.1% | 69.0% | 89.7% | 58.6% | 18537.0 ms | 49809 ms |

#### Tool reliability by confidence band

| band | cases | mean confidence | empirical accuracy |
| --- | ---: | ---: | ---: |
| [0.70, 0.90) | 11 | 81.4% | 81.8% |
| [0.90, 1.00] | 18 | 96.6% | 100.0% |

#### Tool threshold candidates

| threshold | coverage | cases | tool accuracy |
| ---: | ---: | ---: | ---: |
| 0.50 | 100.0% | 29 | 93.1% |
| 0.60 | 100.0% | 29 | 93.1% |
| 0.70 | 100.0% | 29 | 93.1% |
| 0.75 | 100.0% | 29 | 93.1% |
| 0.80 | 86.2% | 25 | 100.0% |
| 0.90 | 62.1% | 18 | 100.0% |
| 0.95 | 51.7% | 15 | 100.0% |

### holdout

| cases | tool accuracy | task accuracy | missing-context accuracy | joint accuracy | mean latency | p95 latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 10 | 90.0% | 60.0% | 100.0% | 60.0% | 23829.1 ms | 67006 ms |

#### Tool reliability by confidence band

| band | cases | mean confidence | empirical accuracy |
| --- | ---: | ---: | ---: |
| [0.50, 0.70) | 1 | 68.0% | 100.0% |
| [0.70, 0.90) | 2 | 76.0% | 50.0% |
| [0.90, 1.00] | 7 | 95.1% | 100.0% |

#### Tool threshold candidates

| threshold | coverage | cases | tool accuracy |
| ---: | ---: | ---: | ---: |
| 0.50 | 100.0% | 10 | 90.0% |
| 0.60 | 100.0% | 10 | 90.0% |
| 0.70 | 90.0% | 9 | 88.9% |
| 0.75 | 80.0% | 8 | 100.0% |
| 0.80 | 80.0% | 8 | 100.0% |
| 0.90 | 70.0% | 7 | 100.0% |
| 0.95 | 50.0% | 5 | 100.0% |

## Interpretation guardrails

- Select a production threshold from the tuning rows, then report the held-out result unchanged.
- Do not claim calibration from a sparse band. Each reported band should have enough examples to be meaningful.
- The Soket baseline returns self-reported confidence through prompted JSON; compare its accuracy and latency, but do not treat its confidence as calibrated like a System One probability.

# Anomaly detection

Every per-second metric dimension (see [metrics.md](metrics.md)) gets its own
unsupervised model. Each new sample is flagged anomalous or not before it is
stored, so every chart carries an anomaly ribbon, every query returns an
`anomalyRate` per point, and metric alerts can fire on the anomaly rate
instead of a hand-picked threshold. The implementation lives in
`web-api/paqtra-metrics/src/anomaly` and uses no ML library.

## How a model works

The approach follows Netdata's ML:

1. **Features.** Recent raw values are differenced (so a steady level or
   slope looks normal), smoothed with a 3-sample moving average and turned
   into lagged vectors of 6 values.
2. **Training.** k-means with k=2 over up to 3,600 vectors from the training
   window (6 hours, clipped to what is stored). The threshold is the 99th
   percentile of the training vectors' distance to their nearest centre.
3. **Scoring.** A sample is anomalous when its vector is farther from both
   centres than the threshold.

A model needs at least 300 vectors (about five minutes). Young models retrain
every 10 minutes, mature ones every 3 hours, at most 200 models per cycle.

About 1% of normal samples exceed the threshold by construction, so a single
bit means little. What matters is the **anomaly rate**: the percentage of
samples flagged in a window. Rollup tiers keep the anomalous count per bucket,
so the rate stays available over long windows.

## Where it runs

- **Agents** score their own node's metrics (`PAQTRA_METRICS_ANOMALY`, on by
  default) before streaming.
- **The API** scores the Hubble-derived series it produces itself.

Models live in memory and are retrained after a restart.

## Querying

`GET /api/v1/metrics/anomalies?after=&before=&nodes=&top=` returns
`summary.nodes` (per-node anomaly rate, scored and anomalous dimensions and a
timeline) and `summary.ranked` (most anomalous dimensions), plus detector
stats.

`GET /api/v1/metrics/correlations?after=&before=&nodes=&top=` answers "what
changed here?" for a window: it ranks dimensions whose value distribution in
the window differs most from the four windows before it (two-sample
Kolmogorov–Smirnov), including metrics no model flagged.

`after` and `before` take Unix seconds or negative seconds relative to now.

```bash
paqtra metrics anomalies --after -3600 --top 20
```

In the console, **Metric Anomalies** shows the per-node timeline; drag across
it to run the correlation for that range.

## Alerting on anomalies

The `anomaly-rate` lookup and the `$anomaly_rate` variable expose the rate to
rules:

```yaml
- alarm: network_anomaly_rate
  on: net.net
  lookup: anomaly-rate -10m
  units: "%"
  warn: '$this > (($status >= $WARNING) ? 20 : 40)'
  delay_down: 15m
```

The built-ins `node_anomaly_rate` and `network_anomaly_rate` do this for CPU
and interface traffic; see [metric-alerts.md](metric-alerts.md).

## Limits

- Models are univariate. Cross-metric correlation comes from the ranked and
  KS views.
- Seasonality longer than the training window (a daily batch job) looks
  anomalous the first time each day.

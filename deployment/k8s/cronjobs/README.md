# Backtesting Engine CronJobs

This directory contains Kubernetes CronJob definitions for automated backtesting tasks.

## Overview

The CronJobs in this directory demonstrate how to schedule automated backtests by calling the Backtesting Engine API endpoints. These jobs use lightweight curl containers to submit HTTP requests to the service.

## Available CronJobs

### 1. Nightly Validation (`nightly-validation.yaml`)

**Schedule:** Daily at 6:00 AM UTC  
**Purpose:** Runs a validation backtest to ensure the system is functioning correctly

**Configuration:**
- Symbol: BTC-USD
- Exchange: Binance
- Standard parameters for consistency
- Keeps last 3 successful and 3 failed job histories

**Usage:**
```bash
kubectl apply -f nightly-validation.yaml
```

### 2. Weekly Optimization (`weekly-optimization.yaml`)

**Schedule:** Sundays at 2:00 AM UTC  
**Purpose:** Performs parameter sweep optimization to find optimal strategy parameters

**Configuration:**
- Tests 5 risk_aversion values: 0.3, 0.4, 0.5, 0.6, 0.7
- Tests 3 inventory_target values: 0.4, 0.5, 0.6
- Submits 15 total backtests (5 × 3 combinations)
- 2-second delay between submissions

**Usage:**
```bash
kubectl apply -f weekly-optimization.yaml
```

### 3. Data Quality Check (`data-quality-check.yaml`)

**Schedule:** Every 30 minutes  
**Purpose:** Validates data availability and system health with quick test backtests

**Configuration:**
- Symbol: ETH-USD
- Runs every 30 minutes
- 5-minute timeout per job
- Replaces previous running job if still active

**Usage:**
```bash
kubectl apply -f data-quality-check.yaml
```

## Prerequisites

### 1. Service Deployment

Ensure the backtesting engine service is deployed and accessible:

```bash
# Check service
kubectl get svc backtesting-engine-service -n backtesting-engine

# Expected output:
# NAME                          TYPE        CLUSTER-IP      EXTERNAL-IP   PORT(S)    AGE
# backtesting-engine-service    ClusterIP   10.96.100.100   <none>        8080/TCP   5d
```

### 2. Namespace

Create the namespace if it doesn't exist:

```bash
kubectl create namespace backtesting-engine
```

### 3. API Endpoint

The CronJobs expect the service to be available at:
```
http://backtesting-engine-service:8080
```

If your service has a different name or port, update the `API_URL` environment variable in each CronJob YAML.

## Deployment

### Deploy All CronJobs

```bash
kubectl apply -f nightly-validation.yaml
kubectl apply -f weekly-optimization.yaml
kubectl apply -f data-quality-check.yaml
```

### Verify Deployment

```bash
# List all CronJobs
kubectl get cronjobs -n backtesting-engine

# Expected output:
# NAME                         SCHEDULE       SUSPEND   ACTIVE   LAST SCHEDULE   AGE
# nightly-validation-backtest  0 6 * * *      False     0        <none>          10s
# weekly-optimization          0 2 * * 0      False     0        <none>          10s
# data-quality-check           */30 * * * *   False     0        <none>          10s
```

### Check CronJob Status

```bash
# View CronJob details
kubectl describe cronjob nightly-validation-backtest -n backtesting-engine

# View jobs created by CronJob
kubectl get jobs -n backtesting-engine --selector=cronjob=nightly-validation-backtest

# View logs from a specific job
kubectl logs job/nightly-validation-backtest-28347200 -n backtesting-engine
```

## Manual Trigger

To manually trigger a CronJob without waiting for the schedule:

```bash
# Create a one-time job from the CronJob
kubectl create job --from=cronjob/nightly-validation-backtest manual-test -n backtesting-engine

# Watch the job
kubectl get job manual-test -n backtesting-engine -w

# View logs
kubectl logs job/manual-test -n backtesting-engine
```

## Customization

### Modify Schedule

The schedule uses standard cron syntax:

```
┌───────────── minute (0 - 59)
│ ┌───────────── hour (0 - 23)
│ │ ┌───────────── day of month (1 - 31)
│ │ │ ┌───────────── month (1 - 12)
│ │ │ │ ┌───────────── day of week (0 - 6) (Sunday to Saturday)
│ │ │ │ │
* * * * *
```

Examples:
- `0 6 * * *` - Every day at 6 AM
- `0 2 * * 0` - Every Sunday at 2 AM
- `*/30 * * * *` - Every 30 minutes
- `0 */4 * * *` - Every 4 hours
- `0 0 1 * *` - First day of every month at midnight

### Modify Backtest Parameters

Edit the JSON payload in the curl command:

```yaml
curl -X POST "${API_URL}/api/backtest" \
  -H "Content-Type: application/json" \
  -d '{
    "symbol": "YOUR-SYMBOL",
    "exchange": "YOUR-EXCHANGE",
    "risk_aversion": 0.5,
    "inventory_target": 0.5,
    "order_size": 0.01,
    "initial_capital": 100000,
    "commission_rate": 0.001,
    "start_date": "2024-01-01T00:00:00Z",  # Optional
    "end_date": "2024-12-31T23:59:59Z"     # Optional
  }'
```

### Change API URL

Update the `API_URL` environment variable:

```yaml
env:
- name: API_URL
  value: "http://your-service-name:your-port"
```

## Monitoring

### View Recent Executions

```bash
# Get all jobs from CronJobs
kubectl get jobs -n backtesting-engine

# View failed jobs
kubectl get jobs -n backtesting-engine --field-selector status.successful=0

# View successful jobs
kubectl get jobs -n backtesting-engine --field-selector status.successful=1
```

### Check Logs

```bash
# Get latest job from CronJob
LATEST_JOB=$(kubectl get jobs -n backtesting-engine \
  --selector=cronjob=nightly-validation-backtest \
  --sort-by=.metadata.creationTimestamp \
  -o jsonpath='{.items[-1].metadata.name}')

# View logs
kubectl logs job/$LATEST_JOB -n backtesting-engine
```

### Suspend/Resume CronJobs

```bash
# Suspend (pause) a CronJob
kubectl patch cronjob nightly-validation-backtest -n backtesting-engine \
  -p '{"spec":{"suspend":true}}'

# Resume a CronJob
kubectl patch cronjob nightly-validation-backtest -n backtesting-engine \
  -p '{"spec":{"suspend":false}}'
```

## Troubleshooting

### Job Fails Immediately

1. Check if the service is accessible:
   ```bash
   kubectl run -it --rm debug --image=curlimages/curl --restart=Never -n backtesting-engine -- \
     curl http://backtesting-engine-service:8080/health
   ```

2. Verify the service endpoint:
   ```bash
   kubectl get svc backtesting-engine-service -n backtesting-engine
   ```

3. Check pod logs:
   ```bash
   kubectl logs -l app=backtesting-engine -n backtesting-engine
   ```

### CronJob Not Executing

1. Check CronJob status:
   ```bash
   kubectl get cronjob nightly-validation-backtest -n backtesting-engine -o yaml
   ```

2. Verify the schedule is correct and not suspended:
   ```bash
   kubectl describe cronjob nightly-validation-backtest -n backtesting-engine
   ```

3. Check for resource limits or quota issues:
   ```bash
   kubectl describe resourcequota -n backtesting-engine
   ```

### Too Many Failed Jobs

Clean up old jobs:

```bash
# Delete all failed jobs older than 1 day
kubectl get jobs -n backtesting-engine --field-selector status.successful=0 \
  -o json | jq -r '.items[] | select(.status.startTime < (now - 86400 | strftime("%Y-%m-%dT%H:%M:%SZ"))) | .metadata.name' | \
  xargs -I {} kubectl delete job {} -n backtesting-engine
```

## Best Practices

1. **Resource Limits**: Add resource requests/limits to prevent cluster resource exhaustion:
   ```yaml
   resources:
     requests:
       memory: "64Mi"
       cpu: "100m"
     limits:
       memory: "128Mi"
       cpu: "200m"
   ```

2. **Concurrency Policy**: Use appropriate concurrency policy:
   - `Forbid`: Don't start if previous job is still running
   - `Replace`: Cancel previous job and start new one
   - `Allow`: Allow concurrent executions

3. **History Limits**: Keep reasonable history to avoid cluttering:
   ```yaml
   successfulJobsHistoryLimit: 3
   failedJobsHistoryLimit: 3
   ```

4. **Timeouts**: Set deadlines to prevent hung jobs:
   ```yaml
   activeDeadlineSeconds: 600  # 10 minutes
   ```

5. **Backoff**: Configure retry behavior:
   ```yaml
   backoffLimit: 2  # Retry up to 2 times
   ```

## Integration with CI/CD

### GitOps Approach

1. Store CronJob definitions in Git
2. Use ArgoCD or Flux to automatically deploy changes
3. Monitor CronJob executions in your GitOps dashboard

### Example ArgoCD Application

```yaml
apiVersion: argoproj.io/v1alpha1
kind: Application
metadata:
  name: backtesting-cronjobs
  namespace: argocd
spec:
  project: default
  source:
    repoURL: https://github.com/your-org/trading-platform
    targetRevision: main
    path: BacktestingEngine/k8s/cronjobs
  destination:
    server: https://kubernetes.default.svc
    namespace: backtesting-engine
  syncPolicy:
    automated:
      prune: true
      selfHeal: true
```

## Additional Resources

- [Kubernetes CronJob Documentation](https://kubernetes.io/docs/concepts/workloads/controllers/cron-jobs/)
- [Cron Expression Generator](https://crontab.guru/)
- [kubectl CronJob Cheat Sheet](https://kubernetes.io/docs/reference/kubectl/cheatsheet/#interacting-with-cronjobs)

## Support

For issues or questions:
1. Check service logs: `kubectl logs -l app=backtesting-engine -n backtesting-engine`
2. Verify API endpoint: `kubectl get svc -n backtesting-engine`
3. Test API manually: `kubectl run -it --rm debug --image=curlimages/curl --restart=Never`

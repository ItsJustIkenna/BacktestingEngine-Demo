# BacktestingEngine Deployment on Self-Hosted Runner

This guide explains how to deploy the BacktestingEngine on the same self-hosted Kubernetes runner as the DataEngine, sharing the same database and Azure secrets.

## Prerequisites

- DataEngine already deployed and running
- Access to the same Azure secrets (DATABASE_URL, Elasticsearch credentials)
- kubectl access to the cluster
- Helm 3.x installed

## Architecture

The BacktestingEngine is configured to:
- **Run on the same node** as DataEngine using `nodeSelector: kubernetes.io/hostname: "self-hosted-runner"`
- **Share the PostgreSQL database** using the same `DATABASE_URL` secret
- **Use the same Elasticsearch logging** with shared Azure credentials
- **Optionally use Redis** for job queue (can be disabled for PostgreSQL-only mode)

## Deployment Steps

### 1. Export Secrets (Same as DataEngine)

```bash
# Database connection (shared with DataEngine)
export POSTGRES_URL="postgresql://user:password@host:5432/database"

# Elasticsearch logging (shared with DataEngine)
export ELASTIC_CLOUD_ENDPOINT="https://your-deployment.es.cloud.io:9243"
export ELASTIC_CLOUD_USERNAME="elastic"
export ELASTIC_CLOUD_PASSWORD="your-elastic-password"

# Optional: API keys (if needed)
export KRAKEN_API_KEY="your-kraken-api-key"
export KRAKEN_SECRET_KEY="your-kraken-secret"
```

### 2. Deploy with Shared Secrets

```bash
cd BacktestingEngine/k8s/backtesting-engine-helm

helm upgrade --install backtesting-engine . \
  --namespace backtesting-engine \
  --create-namespace \
  --set nodeSelector."kubernetes\.io/hostname"="self-hosted-runner" \
  --set secrets.database.url="$POSTGRES_URL" \
  --set secrets.elasticsearch.enabled=true \
  --set secrets.elasticsearch.endpoint="$ELASTIC_CLOUD_ENDPOINT" \
  --set secrets.elasticsearch.username="$ELASTIC_CLOUD_USERNAME" \
  --set secrets.elasticsearch.password="$ELASTIC_CLOUD_PASSWORD" \
  --set replicaCount=1 \
  --set image.repository=backtesting-engine \
  --set image.tag=latest \
  --set image.pullPolicy=IfNotPresent
```

### 3. Verify Deployment

```bash
# Check pod is running on self-hosted runner
kubectl get pods -n backtesting-engine -o wide

# Expected output shows hostname matching DataEngine:
# NAME                                  READY   STATUS    NODE
# backtesting-engine-xxxxxxxxxx-xxxxx   1/1     Running   self-hosted-runner

# Check logs for database connection
kubectl logs -n backtesting-engine -l app.kubernetes.io/name=backtesting-engine --tail=50

# Verify DATABASE_URL is set correctly
kubectl exec -n backtesting-engine deployment/backtesting-engine -- env | grep DATABASE_URL
```

### 4. Test API Endpoints

```bash
# Port forward to access API
kubectl port-forward -n backtesting-engine svc/backtesting-engine 8080:8080

# Test health endpoint
curl http://localhost:8080/health

# Test stats endpoint
curl http://localhost:8080/api/stats

# Submit a test backtest
curl -X POST http://localhost:8080/api/backtest \
  -H "Content-Type: application/json" \
  -d '{
    "symbol": "BTC-USD",
    "exchange": "Binance",
    "risk_aversion": 0.5,
    "inventory_target": 0.5,
    "order_size": 0.01,
    "initial_capital": 100000,
    "commission_rate": 0.001
  }'
```

## Configuration Options

### Node Selection

The `nodeSelector` ensures the pod runs on the same node as DataEngine:

```yaml
nodeSelector:
  kubernetes.io/hostname: "self-hosted-runner"
```

To deploy on a different node, override at deployment:
```bash
--set nodeSelector."kubernetes\.io/hostname"="your-node-name"
```

### Database Configuration

**Shared Database (Recommended):**
```bash
# Use the same DATABASE_URL as DataEngine
--set secrets.database.url="$POSTGRES_URL"
```

**Separate Database:**
```bash
# Use built-in PostgreSQL chart
--set postgresql.enabled=true \
--set secrets.database.url=""
```

### Elasticsearch Logging

**Enable (Shared with DataEngine):**
```bash
--set secrets.elasticsearch.enabled=true \
--set secrets.elasticsearch.endpoint="$ELASTIC_CLOUD_ENDPOINT" \
--set secrets.elasticsearch.username="$ELASTIC_CLOUD_USERNAME" \
--set secrets.elasticsearch.password="$ELASTIC_CLOUD_PASSWORD"
```

**Disable (File logging only):**
```bash
--set secrets.elasticsearch.enabled=false
```

### Redis Job Queue

**PostgreSQL-only mode (Default):**
```bash
--set redis.enabled=false
```

**Enable Redis:**
```bash
--set redis.enabled=true \
--set redis.auth.password="your-redis-password"
```

## Resource Allocation

Default resources are set for shared runner environment:

```yaml
resources:
  limits:
    cpu: 1000m
    memory: 2Gi
  requests:
    cpu: 500m
    memory: 1Gi
```

Adjust based on available resources:
```bash
--set resources.limits.cpu=2000m \
--set resources.limits.memory=4Gi \
--set resources.requests.cpu=1000m \
--set resources.requests.memory=2Gi
```

## Persistence

**Enable persistent storage for results:**
```bash
--set persistence.enabled=true \
--set persistence.size=20Gi
```

**Use existing storage class:**
```bash
--set persistence.storageClass="local-path"
```

## Multi-Instance Deployment

To run multiple replicas on the same runner:

```bash
--set replicaCount=2 \
--set affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].weight=100 \
--set affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm.labelSelector.matchExpressions[0].key="app.kubernetes.io/name" \
--set affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm.labelSelector.matchExpressions[0].operator="In" \
--set affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm.labelSelector.matchExpressions[0].values[0]="backtesting-engine" \
--set affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[0].podAffinityTerm.topologyKey="kubernetes.io/hostname"
```

## Monitoring

### View Logs

```bash
# All pods
kubectl logs -n backtesting-engine -l app.kubernetes.io/name=backtesting-engine -f

# Specific pod
kubectl logs -n backtesting-engine backtesting-engine-xxxxxxxxxx-xxxxx -f

# Previous container (if crashed)
kubectl logs -n backtesting-engine backtesting-engine-xxxxxxxxxx-xxxxx --previous
```

### Check Resource Usage

```bash
# Pod resource usage
kubectl top pods -n backtesting-engine

# Node resource usage
kubectl top node self-hosted-runner
```

### Database Connection

```bash
# Verify DATABASE_URL connectivity
kubectl exec -n backtesting-engine deployment/backtesting-engine -- \
  sh -c 'psql $DATABASE_URL -c "SELECT version();"'

# Check active connections
kubectl exec -n backtesting-engine deployment/backtesting-engine -- \
  sh -c 'psql $DATABASE_URL -c "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database();"'
```

## Troubleshooting

### Pod Not Scheduling

```bash
# Check if nodeSelector matches available nodes
kubectl get nodes --show-labels | grep hostname

# Check pod events
kubectl describe pod -n backtesting-engine backtesting-engine-xxxxxxxxxx-xxxxx
```

### Database Connection Issues

```bash
# Test DATABASE_URL format
echo $POSTGRES_URL

# Should be: postgresql://user:password@host:5432/database

# Verify secret is created correctly
kubectl get secret -n backtesting-engine backtesting-engine-secret -o yaml

# Check DATABASE_URL key exists and is base64 encoded
kubectl get secret -n backtesting-engine backtesting-engine-secret -o jsonpath='{.data.DATABASE_URL}' | base64 -d
```

### Elasticsearch Logging Not Working

```bash
# Check if USE_ELASTICSEARCH_LOGGING is set
kubectl exec -n backtesting-engine deployment/backtesting-engine -- env | grep ELASTICSEARCH

# Verify Elasticsearch secrets
kubectl get secret -n backtesting-engine backtesting-engine-secret -o jsonpath='{.data.ELASTIC_CLOUD_ENDPOINT}' | base64 -d

# Test Elasticsearch connectivity
kubectl exec -n backtesting-engine deployment/backtesting-engine -- \
  curl -u "$ELASTIC_CLOUD_USERNAME:$ELASTIC_CLOUD_PASSWORD" "$ELASTIC_CLOUD_ENDPOINT/_cluster/health"
```

### Resource Constraints

```bash
# Check if pod is being OOMKilled
kubectl get events -n backtesting-engine --sort-by='.lastTimestamp' | grep OOM

# Increase memory limits
helm upgrade backtesting-engine . \
  --reuse-values \
  --set resources.limits.memory=4Gi
```

## Cleanup

```bash
# Delete deployment (keeps PVC)
helm uninstall backtesting-engine -n backtesting-engine

# Delete namespace and all resources
kubectl delete namespace backtesting-engine
```

## Integration with DataEngine

Both engines share:

| Resource | Shared/Separate | Configuration |
|----------|----------------|---------------|
| **Node** | Shared | `nodeSelector: self-hosted-runner` |
| **Database** | Shared | Same `DATABASE_URL` secret |
| **Elasticsearch** | Shared | Same Azure credentials |
| **Network** | Separate | Different namespaces |
| **API** | Separate | Different ports (DataEngine: 8080, BacktestingEngine: 8080) |

Access both services:
```bash
# DataEngine
kubectl port-forward -n data-engine svc/data-engine 8081:8080

# BacktestingEngine
kubectl port-forward -n backtesting-engine svc/backtesting-engine 8080:8080
```

## Security Considerations

1. **Secrets Management**: Never commit secrets to Git. Use environment variables or secret management tools.
2. **Network Policies**: Consider adding NetworkPolicies to restrict traffic between namespaces.
3. **RBAC**: Ensure service accounts have minimal required permissions.
4. **Database Access**: Use separate database users with appropriate permissions for each service.

## Production Recommendations

1. **Use External Database**: Don't use the built-in PostgreSQL chart in production
2. **Enable Monitoring**: Set up Prometheus/Grafana for metrics
3. **Configure Backups**: Implement database backup strategy
4. **Set Resource Limits**: Prevent resource exhaustion
5. **Use Ingress**: Expose services via Ingress controller with TLS
6. **Enable Autoscaling**: Configure HPA based on CPU/memory usage

## Example Production Deployment

```bash
helm upgrade --install backtesting-engine . \
  --namespace backtesting-engine \
  --create-namespace \
  --set nodeSelector."kubernetes\.io/hostname"="self-hosted-runner" \
  --set secrets.database.url="$POSTGRES_URL" \
  --set secrets.elasticsearch.enabled=true \
  --set secrets.elasticsearch.endpoint="$ELASTIC_CLOUD_ENDPOINT" \
  --set secrets.elasticsearch.username="$ELASTIC_CLOUD_USERNAME" \
  --set secrets.elasticsearch.password="$ELASTIC_CLOUD_PASSWORD" \
  --set replicaCount=2 \
  --set resources.limits.cpu=2000m \
  --set resources.limits.memory=4Gi \
  --set resources.requests.cpu=1000m \
  --set resources.requests.memory=2Gi \
  --set persistence.enabled=true \
  --set persistence.size=50Gi \
  --set persistence.storageClass="fast-ssd" \
  --set autoscaling.enabled=true \
  --set autoscaling.minReplicas=2 \
  --set autoscaling.maxReplicas=5 \
  --set autoscaling.targetCPUUtilizationPercentage=70 \
  --set monitoring.enabled=true \
  --set monitoring.serviceMonitor.enabled=true \
  --set ingress.enabled=true \
  --set ingress.className=nginx \
  --set ingress.hosts[0].host=backtesting.yourdomain.com \
  --set ingress.hosts[0].paths[0].path=/ \
  --set ingress.hosts[0].paths[0].pathType=Prefix \
  --set ingress.tls[0].secretName=backtesting-tls \
  --set ingress.tls[0].hosts[0]=backtesting.yourdomain.com
```

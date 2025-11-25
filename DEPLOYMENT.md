# Deployment Strategy

## ✅ Recommended: Helm-Based Deployments

All environment deployments should use Helm charts:

### Local Development (Kubernetes)
```bash
# Install with local development values
helm install backtesting-engine ./k8s/backtesting-engine-helm \
  -f ./k8s/backtesting-engine-helm/values-dev-local.yaml \
  --namespace backtesting-dev \
  --create-namespace
```

### Development Cluster
```bash
helm upgrade --install backtesting-engine ./k8s/backtesting-engine-helm \
  -f ./k8s/backtesting-engine-helm/values-dev.yaml \
  --namespace backtesting-dev \
  --create-namespace
```

### Staging
```bash
helm upgrade --install backtesting-engine ./k8s/backtesting-engine-helm \
  -f ./k8s/backtesting-engine-helm/values-staging.yaml \
  --namespace backtesting-staging \
  --create-namespace
```

### Production
```bash
helm upgrade --install backtesting-engine ./k8s/backtesting-engine-helm \
  -f ./k8s/backtesting-engine-helm/values-prod.yaml \
  --namespace backtesting-prod \
  --create-namespace
```

## 🧪 Local Development Without Kubernetes

For developers who don't want to run Kubernetes locally:

### Option 1: Cargo (Rust Only)
```bash
# Backend only - no UI, no containerization
cd program
export DATABASE_URL="postgresql://user:pass@localhost/backtesting"
cargo run --features postgres -- serve --bind 0.0.0.0:8080
```

### Option 2: Docker Compose (Deprecated)
```bash
# Full stack with Docker only
# ⚠️ Use only for quick local testing, not for any deployed environment
docker-compose up
```

**Status**: docker-compose.yml is kept for convenience but should NOT be used for:
- Development clusters
- Staging
- Production
- CI/CD pipelines

## 🗑️ Files to Remove/Deprecate

### Remove Completely:
- `supervisord.conf` - Only relevant for docker-compose, Kubernetes handles process management
- `.env` - Use Kubernetes secrets via Helm values files
- Any manual deployment scripts

### Keep for Local Dev Only:
- `docker-compose.yml` - Add warning that it's for local testing only
- `Dockerfile` - Still needed by Helm chart

## 📋 CI/CD Pipeline

All deployments should use Helm:

```yaml
# .github/workflows/deploy.yml example
jobs:
  deploy-dev:
    steps:
      - name: Deploy to Dev
        run: |
          helm upgrade --install backtesting-engine ./k8s/backtesting-engine-helm \
            -f ./k8s/backtesting-engine-helm/values-dev.yaml \
            --namespace backtesting-dev \
            --wait

  deploy-staging:
    steps:
      - name: Deploy to Staging
        run: |
          helm upgrade --install backtesting-engine ./k8s/backtesting-engine-helm \
            -f ./k8s/backtesting-engine-helm/values-staging.yaml \
            --namespace backtesting-staging \
            --wait

  deploy-prod:
    needs: [deploy-staging]
    steps:
      - name: Deploy to Production
        run: |
          helm upgrade --install backtesting-engine ./k8s/backtesting-engine-helm \
            -f ./k8s/backtesting-engine-helm/values-prod.yaml \
            --namespace backtesting-prod \
            --wait
```

## 🎯 Benefits

**Helm-Only Deployment Strategy:**
- ✅ Single source of truth for all environments
- ✅ Easy rollbacks: `helm rollback backtesting-engine`
- ✅ Version control for infrastructure
- ✅ Environment-specific configurations via values files
- ✅ Built-in PostgreSQL/Redis dependencies
- ✅ Automatic ConfigMaps, Secrets, Services, Ingress
- ✅ Production-ready: HPA, PVC, health checks, RBAC

## 📚 See Also

- Full Helm documentation: `k8s/backtesting-engine-helm/README.md`
- Self-hosted deployment guide: `k8s/DEPLOYMENT_SELF_HOSTED.md`
- Testing scripts now include Helm deployment testing

# Deployment Strategy

The auction service follows BidMart's staging-first progressive promotion strategy.

## Environment Mapping

| Branch | Environment | Platform |
| --- | --- | --- |
| `staging` | Staging | VPS Docker Compose through `bidmart-infrastructure` |
| `main` | Production | VPS Docker Compose through `bidmart-infrastructure` |

## Gate

The service does not dispatch a VPS deployment directly on push. The deployment trigger runs only after the `Continuous Integration` workflow succeeds on `staging` or `main`.

## Promotion Flow

1. Merge auction changes into `staging`.
2. CI runs tests, coverage, and static analysis.
3. If CI succeeds, this repo dispatches `bidmart-infrastructure` to deploy staging.
4. Validate staging through gateway smoke tests, Grafana metrics, and manual bidding flow.
5. Promote the same change to `main`.
6. CI success on `main` dispatches production deployment.

## Rollback

Rollback is done by reverting the bad commit on `main` and letting CI trigger a new production deployment. For urgent rollback, rerun the infrastructure production workflow with a known-good branch.

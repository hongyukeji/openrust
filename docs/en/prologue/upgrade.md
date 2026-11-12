# Upgrade Guide

## Moving from the v0.x source line

Prepare a new checkout and compare its `.env.example` with the application's settings. Move the application's route, provider and configuration code into the current bootstrap layout. Map `APP_SERVER_HOST` to `APP_HOST` and `SERVER_PORT` to `APP_PORT`, and check the public `APP_URL`. Build and deploy the new `main` executable. Source commands run through `./artisan`, and deployed commands run through `./main artisan`. [Application lifecycle](../architecture-concepts/lifecycle.md)

## Preserve application state

Back up the database and persistent storage. Keep the production application key when existing data depends on it. Validate encrypted data, cookies and sessions using a copy of the old environment. Verify the database schema and migration ledger against the current migrations before running them on production data. Run migrations and application-specific data conversions against the copy first. [Migrations](../database/migrations.md) · [Encryption](../security/encryption.md)

## Validate the deployment

Build the new artifact from its lockfile, start it with staging configuration and verify `/up`, application routes, Artisan commands and any queue workers. Check view, translation and public files at their deployed paths. Keep the previous binary and deployment settings available. If a migration is incompatible with the old binary, coordinate rollback with a database restore or a tested reverse migration. [Deployment](../getting-started/deployment.md)

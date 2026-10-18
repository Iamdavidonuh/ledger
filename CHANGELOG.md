# Changelog

## [0.5.0](https://github.com/Iamdavidonuh/ledger/compare/backend-v0.4.0...backend-v0.5.0) (2026-10-18)


### Features

* add manual revert entry endpoint and UI button ([01e1d24](https://github.com/Iamdavidonuh/ledger/commit/01e1d24c977b1ca87313ac0227418b1589dd8cbf))


### Bug Fixes

* **release:** combine into single release PR to avoid manifest merge conflicts ([bd7a85d](https://github.com/Iamdavidonuh/ledger/commit/bd7a85dfe435a8dc0bf6efbd15fe9cf5ea0feab9))
* **release:** exclude frontend path from backend package so frontend-only commits don't bump backend ([610d684](https://github.com/Iamdavidonuh/ledger/commit/610d68423012d19766d13773938ddc64ed6fa73c))

## [0.4.0](https://github.com/Iamdavidonuh/ledger/compare/backend-v0.3.1...backend-v0.4.0) (2026-10-17)


### Features

* **rates:** add open.er-api.com as fallback fiat source for currencies Frankfurter does not cover (e.g. NGN) ([e919418](https://github.com/Iamdavidonuh/ledger/commit/e919418d435718886cf532a4fc515f3c214deb86))


### Bug Fixes

* **frontend:** prefer major fiat as rates base currency, filter account dropdowns by kind ([45ce947](https://github.com/Iamdavidonuh/ledger/commit/45ce9479ae68ee1847d43b6da99244398fc5c999))

## [0.3.1](https://github.com/Iamdavidonuh/ledger/compare/backend-v0.3.0...backend-v0.3.1) (2026-10-11)


### Bug Fixes

* **docker:** install pkg-config and libssl-dev for reqwest native-tls build ([29aa893](https://github.com/Iamdavidonuh/ledger/commit/29aa893c70fb2046d25d5ddebf785d965437e7c2))

## [0.3.0](https://github.com/Iamdavidonuh/ledger/compare/backend-v0.2.0...backend-v0.3.0) (2026-10-11)


### Features

* add entry parts, balance reconciliation, transfer marking, and import merge ([ce79c78](https://github.com/Iamdavidonuh/ledger/commit/ce79c78d7313952c9b3b252a03bbd5cf817653f5))
* add exchange rate fetching and caching for multi-currency support ([bdff94f](https://github.com/Iamdavidonuh/ledger/commit/bdff94fa4f5e0a0e5bac50a50261dd239a0afc5e))
* **frontend:** wire import merge endpoint and compute blended net worth on rates page ([b054317](https://github.com/Iamdavidonuh/ledger/commit/b0543171f95faf86ee2e7da41c49321a8634bcb8))


### Bug Fixes

* **api:** add balance-at endpoint, clear_transfer flag, and unmark-transfer ([9464b50](https://github.com/Iamdavidonuh/ledger/commit/9464b50eee2a106d5ee1b54f7e4defd401b6f9be))

## [0.2.0](https://github.com/Iamdavidonuh/ledger/compare/backend-v0.1.1...backend-v0.2.0) (2026-10-04)


### Features

* **api:** add LEDGER_CORS_ALLOWED_ORIGINS to allow cross-origin frontends, defaulting to no CORS headers ([3df88a8](https://github.com/Iamdavidonuh/ledger/commit/3df88a815458eeccd42917b4cae5dff0a086d3a4))

## [0.1.1](https://github.com/Iamdavidonuh/ledger/compare/backend-v0.1.0...backend-v0.1.1) (2026-10-03)


### Bug Fixes

* Docker build never copied the new root ledger-workspace crate's src/ ([099ed1f](https://github.com/Iamdavidonuh/ledger/commit/099ed1fd3b2640bc05942699647d8bacc6a61be5))


### Documentation

* document that Release-As isn't path-scoped, and force backend back to 0.1.1 ([34e65a0](https://github.com/Iamdavidonuh/ledger/commit/34e65a0e2dbf190fe632d079054c0a78e1b65646))
* **frontend:** replace the unedited sv-create scaffold README ([38b3e14](https://github.com/Iamdavidonuh/ledger/commit/38b3e14795d70bf8810001e302dc2d0dd1464952))

## [0.1.0](https://github.com/Iamdavidonuh/ledger/compare/backend-v0.1.0...backend-v0.1.0) (2026-10-03)


### Bug Fixes

* release-please's Cargo.toml updater can't replace a workspace-inherited version ([ff42987](https://github.com/Iamdavidonuh/ledger/commit/ff42987a5421ba4215c29b424c67b1d6acec27d1))


### Documentation

* adopt Conventional Commits for release-please ([8b3f901](https://github.com/Iamdavidonuh/ledger/commit/8b3f9017409fc00d6cc3dea6b274f30815caa47e))

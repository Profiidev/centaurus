# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.1] - 2026-04-09

### 🐛 Bug Fixes

- Memory usage



## [0.9.0] - 2026-04-09

### 🐛 Bug Fixes

- Added avatar to detailed user info endpoint



## [0.8.0] - 2026-04-07

### 🚀 Features

- Added option to serve metrics over a different port

### 🐛 Bug Fixes

- Feature flags
- Wrong derive
- Metrics middleware panic
- Metrics extraction

### ⚙️ Miscellaneous Tasks

- Enabled renovate lock file maintenance



## [0.7.0] - 2026-04-05

### 🐛 Bug Fixes

- Rename user info to prevent collision



## [0.6.0] - 2026-04-05

### 🚀 Features

- Added version middleware macro
- Added http proxy
- Added virtual host rewrite and gravatar
- Added db invalid jwt, key and settings table
- Added auth
- Added user and group table
- Added auth handling logic
- Added mail endpoints
- Added settings
- Added setup table
- Added setup endpoints
- Added group endpoints
- Added user endpoints
- Added if not exists to index creation
- Added update message derive macro

### 🐛 Bug Fixes

- Naming
- Wrong item paths
- Better endpoint exposure
- User endpoint pub
- Feature inconsistencies
- Version header location

### 🚜 Refactor

- Use trait for proxy
- Backend file structure

### ⚙️ Miscellaneous Tasks

- Updated deps



## [0.5.0] - 2026-04-04

### 🚀 Features

- Implement operation output for error report
- Added aide derive to db
- Added aide derive to all state
- Added rate limiter
- Added config trait
- Use api router when using openapi
- Better support for other service for run app
- Added openapi json route
- Added swagger docs
- Added config derive macro

### 🐛 Bug Fixes

- Aide trait impl
- Router build lifetime

### ⚙️ Miscellaneous Tasks

- Update aide



## [0.4.13] - 2026-03-15

### 🐛 Bug Fixes

- Errors not working without http feature



## [0.4.12] - 2026-03-04

### 🐛 Bug Fixes

- Impl error report macro not working because of http feature



## [0.4.11] - 2026-02-05

### 🚀 Features

- Added separate serde json features



## [0.4.10] - 2026-02-05

### 🚀 Features

- Only pass log level to logging init

### 🐛 Bug Fixes

- Errors without http status



## [0.4.9] - 2026-01-29

### 🚀 Features

- Added run app with connect info



## [0.4.8] - 2026-01-27

### 🚀 Features

- Added url error impl



## [0.4.7] - 2025-12-06

### 🚀 Features

- Add base router setup
- Added metrics and frontend state init
- Added metrics config



## [0.4.6] - 2025-11-27

### 🐛 Bug Fixes

- Track caller for closures



## [0.4.5] - 2025-11-27

### 🐛 Bug Fixes

- Added missing track caller



## [0.4.4] - 2025-11-03

### 🐛 Bug Fixes

- Updated axum extra



## [0.4.3] - 2025-11-02

### 🚀 Features

- Docker error impl



## [0.4.2] - 2025-10-30

### 🚀 Features

- Impl std error for error report
- Added kube rs error impl



## [0.4.1] - 2025-10-30

### 🚀 Features

- Added state extract method

### 🚜 Refactor

- Replace custom from request derive with official



## [0.4.0] - 2025-10-23

### 🚀 Features

- Added db init
- Added auth utils
- Added health route
- Added new error types
- Added redirect

### 🐛 Bug Fixes

- Webauthn rs error
- Error impl
- Error impl

### 🚜 Refactor

- Moved serde fns



## [0.3.2] - 2025-10-12

### 🚀 Features

- Custom error status
- Added context option for errors without setting status

### 🚜 Refactor

- Log user error only as warning and not error



## [0.3.1] - 2025-10-11

### 🚀 Features

- Added metrics
- Added option for extra labels

### 🐛 Bug Fixes

- Instrument



## [0.3.0] - 2025-10-10

### 🚀 Features

- Added optional filter method for request logging

### 🐛 Bug Fixes

- Correct uri usage in response handler

### 🚜 Refactor

- Removed unnessacary reexports

### 🧪 Testing

- Debug field
- Debug field



## [0.2.4] - 2025-10-09

### 🚀 Features

- Made config figment compatible



## [0.2.3] - 2025-10-07

### 🚀 Features

- New error from



## [0.2.2] - 2025-10-06

### 🐛 Bug Fixes

- Jsonwebtoken



## [0.2.1] - 2025-10-05

### 🚀 Features

- Xml into response



## [0.2.0] - 2025-09-30

### 🚜 Refactor

- Less feature flags



## [0.1.0](https://github.com/Profiidev/centaurus/releases/tag/centaurus-v0.1.0) - 2025-09-28

### Added

- added request utils
- added init code
- initial commit

### Other

- added pipelines
- added missing features

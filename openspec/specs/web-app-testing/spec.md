### Requirement: Web test harness

The `apps/web` frontend SHALL have a test harness based on `vitest` with `jsdom` and `@testing-library/react`, runnable via a `test` script in `package.json`.

#### Scenario: Run web tests
- **WHEN** a developer runs `pnpm test` (or `pnpm test --run`) in `apps/web`
- **THEN** vitest SHALL execute all test files in a jsdom environment and report pass/fail

#### Scenario: Component rendering
- **WHEN** a React component is rendered in a test via `@testing-library/react`
- **THEN** the test SHALL be able to query the rendered DOM and assert with `@testing-library/jest-dom` matchers

### Requirement: Initial web test coverage

The web app SHALL include seed tests for pure utilities and hooks that encode core logic.

#### Scenario: Utility tests
- **WHEN** the web test suite runs
- **THEN** it SHALL include tests for `utils/pkce.ts` (challenge/verifier generation) and `utils/api.ts` (request construction/parsing)

#### Scenario: Hook tests
- **WHEN** the web test suite runs
- **THEN** it SHALL include at least one hook test (e.g. `useXAuth`) exercising its logic with mocked network calls

### Requirement: End-to-end browser tests

The web app SHALL have a Playwright (`@playwright/test`) E2E suite that drives a real browser against a served production build, runnable via a `test:e2e` script. The suite SHALL boot the frontend automatically (Playwright `webServer`) and SHALL mock backend API responses so it does not depend on live backend infrastructure.

#### Scenario: Run E2E suite
- **WHEN** a developer runs `pnpm test:e2e` in `apps/web`
- **THEN** Playwright SHALL build/serve the app, launch a browser, execute the E2E specs, and report pass/fail

#### Scenario: Backend calls are intercepted
- **WHEN** an E2E test triggers a frontend action that calls the backend API
- **THEN** the request SHALL be intercepted and answered with a canned response, with no call to a live backend

### Requirement: Critical-flow E2E coverage

The E2E suite SHALL cover the critical user-facing flows of the app.

#### Scenario: Home and navigation
- **WHEN** a user opens the home page in an E2E test
- **THEN** the page SHALL render and navigation to onboarding/dashboard SHALL work

#### Scenario: OAuth callback flow
- **WHEN** the OAuth callback route is loaded with a mocked token-exchange response
- **THEN** the app SHALL complete the callback and route the user to the authenticated view

#### Scenario: Dashboard / account view
- **WHEN** an authenticated user views the dashboard or account page with mocked balance/account data
- **THEN** the expected account and balance information SHALL be displayed

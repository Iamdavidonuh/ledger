// Runtime config for the static SPA build, read by api/client.ts. A
// deployment overwrites this file (see the Helm chart's init container)
// before the app's own bundle runs; this checked-in copy is only the
// local-dev default, and names no real host on purpose.
window.__ENV__ = {
	API_BASE_URL: '/api'
};

// A pure client-rendered SPA against the Axum API: the build never needs
// the backend reachable, and adapter-static's fallback serves this same
// shell for every route.
export const ssr = false;

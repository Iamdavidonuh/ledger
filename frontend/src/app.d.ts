// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}

	// Set by /config.js (see app.html), loaded before this app's own bundle
	// runs. See api/client.ts.
	interface Window {
		__ENV__?: {
			API_BASE_URL?: string;
		};
	}
}

export {};
